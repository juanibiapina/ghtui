Output ../../public/custom-commands.webm
Output ../../public/custom-commands.mp4

Set Theme TokyoNight
Set Padding 0
Set FontSize 24
Set FontFamily "CommitMono Nerd Font Mono"
Set Width 1400
Set Height 600

Hide
Type "DEMO=$PWD; cd $(mktemp -d) && git clone -q https://github.com/juanibiapina/ghtui && cd ghtui && clear && ghtui --config $DEMO/custom-commands.yml"

Enter
Sleep 15s
Show

Sleep 1000ms

Type "g"
Sleep 1000ms
Type@200ms "jjjjjjjjjj"
Sleep 1000ms
Type "q"

