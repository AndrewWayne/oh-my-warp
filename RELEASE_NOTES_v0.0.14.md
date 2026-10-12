# oh my warp 0.0.14 (Windows Pre-release)

Windows application behavior, dependencies and packaging scripts are unchanged
from the accepted 0.0.13 Windows source. This build updates the version label to
0.0.14 and repeats portable-package and installer acceptance on a Windows runner.
The new Mac Codex path adaptation is not applied to Windows.

Windows 10 or newer, x64 only. Run the per-user installer, or extract the entire
portable ZIP and start `omw-warp-oss.exe`. Keep Node, the Agent, Credential Manager
helper, OpenConsole and runtime DLLs with the executable. Each downloadable
package has a SHA-256 sidecar.

The release platform manifest identifies this independent Windows source and
its build evidence. Manual Windows desktop interaction and older operating-system
execution remain unverified. This is a public Pre-release, not a stable release.
