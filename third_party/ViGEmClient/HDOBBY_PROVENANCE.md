# ViGEmClient provenance

- Upstream: <https://github.com/nefarius/ViGEmClient>
- Reviewed revision: `b66d02d57e32cc8595369c53418b843e958649b4`
- License: MIT; see `LICENSE` in this directory.
- Included files: the public headers and `ViGEmClient.cpp` needed for the
  statically linked Windows user-mode client.
- Driver: not included. ViGEmClient requires a separately installed compatible
  ViGEmBus driver.

The upstream project is retired. HdobbyDesk keeps this backend optional and
must not enable Windows test-signing or install an unsigned driver.
