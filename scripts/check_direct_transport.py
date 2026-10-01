#!/usr/bin/env python3
"""Measure an explicitly supplied HdobbyDesk direct TLS endpoint without saving it."""

import argparse
from collections import Counter
import concurrent.futures
import hashlib
import re
import socket
import ssl
import statistics
import time


ALPN = "hdobby-direct/1"
SERVER_NAME = "hdobby.direct"
FINGERPRINT = re.compile(r"^[0-9a-f]{64}$")


def bounded_int(name, minimum, maximum):
    def convert(value):
        try:
            parsed = int(value)
        except ValueError as error:
            raise argparse.ArgumentTypeError(f"{name} must be an integer") from error
        if not minimum <= parsed <= maximum:
            raise argparse.ArgumentTypeError(
                f"{name} must be between {minimum} and {maximum}"
            )
        return parsed

    return convert


def normalize_fingerprint(value):
    normalized = re.sub(r"[\s:]", "", value).lower()
    if not FINGERPRINT.fullmatch(normalized):
        raise argparse.ArgumentTypeError(
            "fingerprint must contain exactly 64 hexadecimal SHA-256 digits"
        )
    return normalized


def parse_args():
    parser = argparse.ArgumentParser(
        description=(
            "Run an opt-in TLS 1.3 reachability measurement. Target and certificate "
            "values are required at runtime and are never printed or written to disk."
        )
    )
    parser.add_argument("--host", required=True, help="explicit host name or IP address")
    parser.add_argument(
        "--port", required=True, type=bounded_int("port", 1, 65535)
    )
    parser.add_argument(
        "--fingerprint", required=True, type=normalize_fingerprint,
        help="expected peer certificate SHA-256 fingerprint",
    )
    parser.add_argument(
        "--attempts", type=bounded_int("attempts", 1, 1000), default=40
    )
    parser.add_argument(
        "--concurrency", type=bounded_int("concurrency", 1, 16), default=1
    )
    parser.add_argument(
        "--interval-ms", type=bounded_int("interval-ms", 0, 10000), default=250
    )
    parser.add_argument(
        "--timeout-ms", type=bounded_int("timeout-ms", 100, 30000), default=3000
    )
    args = parser.parse_args()
    if not args.host.strip() or any(char.isspace() for char in args.host):
        parser.error("host must be a non-empty address without whitespace")
    if any(char in args.host for char in "/@[]"):
        parser.error("host must not be a URL, user-info value, or bracketed address")
    return args


def context():
    tls = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
    tls.minimum_version = ssl.TLSVersion.TLSv1_3
    tls.maximum_version = ssl.TLSVersion.TLSv1_3
    tls.check_hostname = False
    tls.verify_mode = ssl.CERT_NONE
    tls.set_alpn_protocols([ALPN])
    return tls


def connect_once(args):
    # Context construction is local CPU work, not transport latency.
    tls = context()
    started = time.perf_counter()
    try:
        with socket.create_connection(
            (args.host, args.port), timeout=args.timeout_ms / 1000
        ) as raw:
            tcp_done = time.perf_counter()
            raw.settimeout(args.timeout_ms / 1000)
            with tls.wrap_socket(raw, server_hostname=SERVER_NAME) as stream:
                tls_done = time.perf_counter()
                certificate = stream.getpeercert(binary_form=True)
                if not certificate:
                    return "missing_certificate", None
                if hashlib.sha256(certificate).hexdigest() != args.fingerprint:
                    return "certificate_mismatch", None
                if stream.version() != "TLSv1.3":
                    return "tls_version_mismatch", None
                if stream.selected_alpn_protocol() != ALPN:
                    return "alpn_mismatch", None
                return "ok", (
                    (tcp_done - started) * 1000,
                    (tls_done - tcp_done) * 1000,
                    (tls_done - started) * 1000,
                )
    except (TimeoutError, socket.timeout):
        return "timeout", None
    except ssl.SSLError:
        return "tls_error", None
    except OSError:
        return "network_error", None


def percentile(values, fraction):
    index = max(0, min(len(values) - 1, int(len(values) * fraction) - 1))
    return values[index]


def main():
    args = parse_args()
    results = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.concurrency) as pool:
        for start in range(0, args.attempts, args.concurrency):
            count = min(args.concurrency, args.attempts - start)
            results.extend(pool.map(lambda _: connect_once(args), range(count)))
            if start + count < args.attempts and args.interval_ms:
                time.sleep(args.interval_ms / 1000)

    timings = [timing for status, timing in results if status == "ok"]
    failures = Counter(status for status, _ in results if status != "ok")
    print(
        f"attempts={len(results)} success={len(timings)} "
        f"failure={len(results) - len(timings)}"
    )
    if timings:
        for index, name in enumerate(("tcp_ms", "tls_ms", "total_ms")):
            values = sorted(row[index] for row in timings)
            print(
                f"{name}: min={min(values):.1f} "
                f"p50={statistics.median(values):.1f} "
                f"p95={percentile(values, 0.95):.1f} "
                f"max={max(values):.1f}"
            )
    if failures:
        print("failure_types=" + ",".join(f"{key}:{value}" for key, value in sorted(failures.items())))
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
