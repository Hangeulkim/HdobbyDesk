# Collaborative input

HdobbyDesk has two opt-in input modes for sessions where several people connect
to the same host.

## Collaborative cursor control

Each connection keeps its own named presence cursor. Moving a presence cursor
does not move the operating system cursor. Clicking first moves the system
cursor to that participant's last presence position and presses the button
under the same input lock. A drag owns the system cursor until all of that
participant's buttons are released. Other participants can keep moving their
presence cursors while the drag is active, but their click and drag actions are
held back.

The gesture lease expires after ten seconds of inactivity and connection
cleanup releases every held button. Relative mouse mode remains a separate
single-controller mode because relative movement has no absolute presence
position.

Windows, macOS, and Linux desktops expose only one ordinary system cursor and
keyboard focus to normal applications. Independent simultaneous drags require
application-specific collaboration, annotations, multi-touch support, or
separate application sessions; the remote desktop transport cannot add that
semantic to an arbitrary application.

## Physical keyboard as a separate gamepad

On a Windows host, each connection can convert its physical keyboard into a
separate virtual Xbox 360 controller. The host assigns the XInput user index;
the remote participant cannot select or replace another connection's slot. A
maximum of four XInput controllers can be active.

The initial mapping is:

| Keyboard | Virtual controller |
| --- | --- |
| W / A / S / D | Left stick |
| Arrow keys | D-pad |
| J / K / U / I | A / B / X / Y |
| Q / E | Left / right bumper |
| Left or right Shift / Space | Left / right trigger |
| 1 / 2 | Back / Start |

Opposite stick or D-pad directions cancel to neutral. Repeated key-down packets
do not create duplicate reports. Enabling this mode consumes all keyboard
events from that connection so an unmapped key cannot type into the Windows
desktop. Turning the mode off or losing the connection sends a neutral report
before removing the virtual controller.

The current compatibility backend statically embeds the MIT-licensed
ViGEmClient and talks to the separately installed, signed ViGEmBus driver. The
driver is never downloaded or installed by a normal connection. ViGEmBus is
widely deployed but reached end of life in 2023, so it is an optional
compatibility backend rather than a long-term trust anchor. Test-signing mode
and unsigned drivers are not supported.

An actively maintained driver should replace this backend before a production
release. Candidate evaluation must include license, signature provenance,
driver isolation, Windows version support, uninstall behavior, and real
four-client input testing. A locally trusted self-signed certificate is not the
same trust model as a Microsoft-signed public driver.

No server address, device identifier, password, certificate, or private key is
part of this feature or its tests.
