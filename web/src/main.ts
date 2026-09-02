/** Wires the transport controls to the streaming player. */
import { Player, type StreamInfo } from "./player.js";

function element<T extends HTMLElement>(id: string): T {
  const node = document.getElementById(id);
  if (!node) throw new Error(`missing element #${id}`);
  return node as T;
}

const ui = {
  dropzone: element<HTMLDivElement>("dropzone"),
  fileInput: element<HTMLInputElement>("file-input"),
  status: element<HTMLParagraphElement>("status"),
  deck: element<HTMLElement>("deck"),
  toggle: element<HTMLButtonElement>("toggle"),
  scrub: element<HTMLInputElement>("scrub"),
  elapsed: element("elapsed"),
  total: element("total"),
  rate: element("fact-rate"),
  channels: element("fact-channels"),
  buffer: element("fact-buffer"),
  dropped: element("fact-dropped"),
};

const SCRUB_MAX = 1000;

let playing = false;
let scrubbing = false;
let duration: number | undefined;

function clock(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return "0:00";
  const total = Math.floor(seconds);
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
}

function setStatus(message: string, tone: "info" | "error" = "info"): void {
  ui.status.textContent = message;
  ui.status.dataset["tone"] = tone;
}

function setPlaying(next: boolean): void {
  playing = next;
  ui.toggle.textContent = next ? "❚❚" : "▶";
  ui.toggle.setAttribute("aria-label", next ? "Pause" : "Play");
}

const player = new Player({
  onInfo(info: StreamInfo) {
    duration = info.durationSeconds;
    ui.deck.hidden = false;
    ui.rate.textContent = `${info.sampleRateHz} Hz`;
    ui.channels.textContent = info.channelCount === 1 ? "mono" : `${info.channelCount}`;
    ui.total.textContent = clock(duration ?? 0);
    ui.scrub.disabled = duration === undefined;
  },
  onProgress(playedSeconds: number, bufferedSeconds: number) {
    ui.elapsed.textContent = clock(playedSeconds);
    ui.buffer.textContent = `${bufferedSeconds.toFixed(2)} s`;
    if (!scrubbing && duration && duration > 0) {
      ui.scrub.value = String(Math.min(SCRUB_MAX, (playedSeconds / duration) * SCRUB_MAX));
    }
  },
  onEnded() {
    setPlaying(false);
    setStatus("Finished.");
  },
  onError(message: string) {
    setStatus(message, "error");
    setPlaying(false);
  },
});

async function handleFile(file: File): Promise<void> {
  setStatus(`Opening ${file.name}…`);
  try {
    const bytes = new Uint8Array(await file.arrayBuffer());
    const info = await player.load(bytes);
    ui.dropped.textContent = "0";
    setStatus(
      `${file.name} — ${info.sampleRateHz} Hz, ` +
        `${info.channelCount === 1 ? "mono" : `${info.channelCount} ch`}` +
        (info.durationSeconds ? `, ${clock(info.durationSeconds)}` : ""),
    );
    await player.play();
    setPlaying(true);
  } catch (error) {
    setStatus(error instanceof Error ? error.message : "could not open that file", "error");
  }
}

ui.toggle.addEventListener("click", () => {
  if (playing) {
    player.pause();
    setPlaying(false);
  } else {
    void player.play();
    setPlaying(true);
  }
});

ui.scrub.addEventListener("pointerdown", () => {
  scrubbing = true;
});
const commitSeek = (): void => {
  if (!scrubbing || !duration) return;
  scrubbing = false;
  player.seek((Number(ui.scrub.value) / SCRUB_MAX) * duration);
};
ui.scrub.addEventListener("pointerup", commitSeek);
ui.scrub.addEventListener("change", commitSeek);

const open = (): void => ui.fileInput.click();
ui.dropzone.addEventListener("click", open);
ui.dropzone.addEventListener("keydown", (event) => {
  if (event.key === "Enter" || event.key === " ") {
    event.preventDefault();
    open();
  }
});
ui.fileInput.addEventListener("change", () => {
  const file = ui.fileInput.files?.[0];
  if (file) void handleFile(file);
});
for (const type of ["dragenter", "dragover"] as const) {
  ui.dropzone.addEventListener(type, (event) => {
    event.preventDefault();
    ui.dropzone.classList.add("is-dragging");
  });
}
for (const type of ["dragleave", "drop"] as const) {
  ui.dropzone.addEventListener(type, () => ui.dropzone.classList.remove("is-dragging"));
}
ui.dropzone.addEventListener("drop", (event) => {
  event.preventDefault();
  const file = event.dataTransfer?.files?.[0];
  if (file) void handleFile(file);
});
