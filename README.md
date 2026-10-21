This program is built to address the issue present in the latest Steam Frame Beta and Main updates which causes external audio devices to not function in programs which use the microphone.
This is achieved by monitoring all changes in pipewire connections, and, upon detecting a disconnected application, automatically stepping in to ensure all programs are properly connected to audio output.


The installer sets this up as a service with systemd, meaning it will be automatically run on every boot. Theoretically, this means the fix will be completely invisible, and once installed it will require no input from you.

---

### Install
```bash
curl -fsSL https://raw.githubusercontent.com/OverlordPotato1/FrameAudioAutoRepatch/master/install.sh | bash
```

### Update
```bash
curl -fsSL https://raw.githubusercontent.com/OverlordPotato1/FrameAudioAutoRepatch/master/update.sh | bash
```


--------
### AI Usage
Places where AI was used: 
- to assist with interacting and understanding pipewire
- debugging
- writing documentation
- install and update scripts
- figuring out how to cross-compile for arm
  
All components that were written by AI were manually retyped and refactored for readbility, to ensure no harmful code was being written, and to intervene on any potential vulnerabilities. I am capable of reading and understanding the behaviour and function of all AI generated code. README as well as any other .md files are written fully without AI assistance.
