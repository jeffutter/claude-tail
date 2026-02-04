Output docs/demo.gif

# Terminal settings
Set FontSize 14
Set Width 1400
Set Height 800
Set Theme "TokyoNight"
Set Margin 10
Set BorderRadius 8
Set WindowBar Colorful
Set TypingSpeed 20ms
Set Framerate 24
Set CursorBlink false

Require target/release/claude-tail
Require tmux
Require claude

Set Shell fish

Hide
	Type "tmux"
	Enter
	Sleep 1s
	Ctrl+A
	Type "%"

	Sleep 1s
	Type "./target/release/claude-tail"
	Enter
	Sleep 1s

	Ctrl+A
	Left
	Type "cd ~/Projects/cowfight/"
	Enter
	Type "claude --permission-mode acceptEdits"
	Enter
	Sleep 30s
Show

# Launch Prompt
Type "Spawn two competing sub-agents with the same task. Each implementation should create a bash script that will output whimsical insults as ascii art spoken by a cow. Do not use colors in the terminal output. Name the files `cow1.sh` and `cow2.sh`. When the two scripts run back-to-back it should look like the cows are arguing. GO!"
Enter
Sleep 5s

# Switch Back
Ctrl+A
Right
Type "r"

# Project Tab 
Sleep 5s
Tab

# Session Tab 
Sleep 3s
Tab

# Agent Tab 
Sleep 500ms
Tab

# Main Conversation
Sleep 8s

Shift+Tab

# Agent Tab
Down
Sleep 1s 
Tab

# Agent Conversation Tab
Sleep 8s 
Shift+Tab

# Agent Tab
Type "g"
Sleep 1s 
Tab

# Main Conversation Tab
Sleep 20s
