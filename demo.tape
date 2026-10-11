# Record the README demo GIF with vhs: https://github.com/charmbracelet/vhs
#
#   cargo build --release
#   vhs demo.tape
#
# It uses --demo, so it replays recorded traffic and TLEs and never shows
# anyone's real location. --time pins a night so the real stars, the
# constellation lines and the Moon are visible.

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

Sleep 5s
# The guide explains the chart without stopping live tracking.
Type "?"
Sleep 4s
Escape
Type "c"
Sleep 3s
Tab
Sleep 4s
Backspace
Type "h"
Sleep 3s
Right
Sleep 3s
# Return overhead, then remove instruments and callsigns for a quiet sky.
Type "hvl"
Sleep 4s
Type "q"
Sleep 1s
