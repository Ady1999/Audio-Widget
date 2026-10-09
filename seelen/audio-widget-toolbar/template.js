if (isMuted) return icon("PiSpeakerXFill");
if (volume < 0.01) return icon("PiSpeakerNoneFill");
if (volume < 0.5) return icon("PiSpeakerLowFill");
return icon("PiSpeakerHighFill");
