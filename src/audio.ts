import sound1 from "./assets/sounds/1.mp3";
import sound2 from "./assets/sounds/2.mp3";
import sound3 from "./assets/sounds/3.mp3";
import sound4 from "./assets/sounds/4.mp3";
import type { SoundId } from "./types";

const sources: Record<SoundId, string> = {
  "1": sound1,
  "2": sound2,
  "3": sound3,
  "4": sound4,
};

const players = Object.fromEntries(
  Object.entries(sources).map(([id, source]) => {
    const player = new Audio(source);
    player.preload = "auto";
    player.volume = 0.6;
    return [id, player];
  }),
) as Record<SoundId, HTMLAudioElement>;

let active: HTMLAudioElement | null = null;

export function preloadSounds(): void {
  for (const player of Object.values(players)) player.load();
}

export async function playSound(id: SoundId): Promise<void> {
  active?.pause();
  active = players[id];
  active.currentTime = 0;
  const current = active;
  try {
    await current.play();
  } finally {
    current.addEventListener("ended", () => {
      if (active === current) active = null;
    }, { once: true });
  }
}
