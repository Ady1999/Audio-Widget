const pct = Math.round((volume || 0) * 100);
return ["Audio: ", isMuted ? "muted" : pct + "%"];
