Output ../../public/sections.webm
Output ../../public/sections.mp4

Set Theme TokyoNight
Set Padding 0
Set FontSize 24
Set FontFamily "CommitMono Nerd Font Mono"
Set Width 1200
Set Height 600

Hide
Type "DEMO=$PWD; cd $(mktemp -d) && git clone -q https://github.com/juanibiapina/ghtui && cd ghtui && clear && ghtui --config $DEMO/sections.yml"

Enter
Sleep 15s
Show

Sleep 1500ms

Type "l"
Sleep 1000ms

Hide
Type "s"
Sleep 8s
Type "l"
Type "h"
Show

Sleep 1500ms

Type "l"
Sleep 1000ms
