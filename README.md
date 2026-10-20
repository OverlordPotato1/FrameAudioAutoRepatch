This program fixes the issue where in the current Steam Frame beta that causes audio to stop working when the microphone is activated.
It does this by automatically scanning for and reconnecting any disconnected audio sources in pipewire every time there is an update to connections.


The installer sets this up as a service with systemd, meaning it will be automatically run on every boot. Theoretically, this means the fix will be completely invisible, and once installed it will require no input from you.

Install
```bash
curl -fsSL https://raw.githubusercontent.com/OverlordPotato1/FrameAudioAutoRepatch/master/install.sh | bash
```
