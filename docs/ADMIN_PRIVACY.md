# Host administration privacy

On Windows 10 version 2004 and later, the top-level HdobbyDesk management
window is marked with `WDA_EXCLUDEFROMCAPTURE`. The local operator can still see
and use it, while compatible Windows capture APIs omit it from the remote video.
This covers the device ID, password, certificate, network, and security pages
shown inside that window.

Both the Windows runner and native core apply the policy. The native core uses a
bounded five-second startup search for only these exact top-level titles:
`HdobbyDesk`, `HdobbyDesk - Connection Manager`, and `HdobbyDesk - Install`.
Remote desktop viewer windows do not match and remain capturable. This also lets
an older compatible Flutter runner gain the protection when its native core is
updated.

The capture exclusion is defense in depth. It does not grant administrator
rights, protect a secret copied into another application, hide Windows Event
Viewer or Registry Editor, or redact arbitrary third-party windows. Selective
redaction of other applications would require a separate capture compositor and
an explicit window policy.

Windows UAC uses the secure desktop. Capturing and controlling a UAC prompt is a
service/elevation concern and is independent from hiding HdobbyDesk's own
management window. HdobbyDesk must not disable UAC or weaken secure-desktop
policy to make the prompt easier to control.

On older Windows versions, `SetWindowDisplayAffinity` can fail. HdobbyDesk keeps
the local management window usable and does not claim capture exclusion in that
case. A production UI should surface the active/failed capture-protection state
to the local host operator.
