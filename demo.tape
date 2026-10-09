# Record the README demo GIF with vhs: https://github.com/charmbracelet/vhs
#
#   cargo build --release
#   vhs demo.tape
#
# It uses --demo, so it replays recorded traffic (and TLEs) around a busy
# airport and never shows anyone's real location. --time pins a night so the
# Moon, planets and satellites are visible.

Output demo.gif

Set Shell bash
Set FontSize 16
Set Width 1100
Set Height 620
Set Padding 12
Set BorderRadius 8
Set CursorBlink false
Set Theme "Catppuccin Mocha"

Type "./target/release/overflight --demo --time 2024-12-21T00:00:00Z"
Enter

Sleep 6s
Type "h"
Sleep 5s
Type "h"
Sleep 3s
Tab
Sleep 4s
Type "q"
Sleep 1s
