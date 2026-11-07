Output "/tmp/ghtui-screenshots.mp4"

Set Theme TokyoNight
Set Padding 0
Set FontSize 24
Set FontFamily "CommitMono Nerd Font Mono"
Set Width 1600
Set Height 826

Hide
Type "DEMO=$PWD; DEMO_REPO=$(mktemp -d)/ghtui; git clone -q https://github.com/juanibiapina/ghtui $DEMO_REPO"
Enter
Sleep 5s

Hide
Type "cd $DEMO_REPO && clear && ghtui --config $DEMO/theme-tokyonight.yml"
Enter
Sleep 15s
Show
Sleep 2s
Type "?"
Sleep 3s
Hide
Type "q"
Sleep 1s

Hide
Type "cd $DEMO_REPO && clear && ghtui --config $DEMO/theme-catpuccin.yml"
Enter
Sleep 15s
Show
Sleep 2s
Hide
Type "q"
Sleep 1s

Hide
Type "cd $DEMO_REPO && clear && ghtui --config $DEMO/theme-gruvbox.yml"
Enter
Sleep 15s
Show
Sleep 2s
Hide
Type "q"
Sleep 1s
