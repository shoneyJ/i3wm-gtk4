### speech-text follow

- the speech-text widget currently available in control panel needs to me moved to an seperate UI. It does not fit into the system control panel but rather is a part of productivity.
- User needs an option to view the whisper model generated German text along with the trans cli translated english text.
- The content generated on persession should be scrollable.
- The live generation should be auto scrolling.
- The application should be launchable from system tray or using i3 key (mod+space) application lists.
- Option to navigate to prevous sessions should be available.

## bugs-to-fix

- UI - change the button text color, as of now they are not visible, keep as per translator app.
  assets/speech-text-ui.css - **not fixed**
  keep the button color darker, not it is white.

- when the application is lauched it starts recording. User not able to type a session name and start manually. **fixed**

## speech-text UI improvement

- split the view in half, top being the German (speech to text), bottom being English using trans cli.
- time stamp is not required in the UI.
- goal is to understand the context of the playing german audio.
