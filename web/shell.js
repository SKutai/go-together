const loader = document.querySelector("#go-together-loader");
const canvas = document.querySelector("#go-together");
const frame = document.querySelector(".canvas-frame");
const shell = document.querySelector(".game-shell");
const shellCopy = document.querySelector(".shell-copy");
const status = document.querySelector("#web-status");
const guidance = document.querySelector("#focus-guidance");
// The smallest window the game's screens are laid out for, in CSS pixels.
const minimumWidth = 1040;
const minimumHeight = 600;

// The canvas fills the page, less the status lines under it, and the game
// lays itself out at the canvas's size: winit reports each change of it as
// a resize. A page smaller than the minimum in either direction gets a
// canvas the minimum's size in that direction, drawn scaled down by one
// factor until it fits, as a desktop window smaller than the minimum is.
// Both directions come from the layout viewport: browser zoom changes it,
// and the game lays itself out again, but a pinch zoom only magnifies the
// page and leaves the game's layout alone.
const layOutCanvas = () => {
  if (!canvas || !frame || !shell || !shellCopy) {
    return;
  }
  const viewportWidth = document.documentElement.clientWidth;
  const viewportHeight = document.documentElement.clientHeight;
  const shellStyle = getComputedStyle(shell);
  const horizontalPadding =
    Number.parseFloat(shellStyle.paddingLeft) + Number.parseFloat(shellStyle.paddingRight);
  const verticalPadding =
    Number.parseFloat(shellStyle.paddingTop) + Number.parseFloat(shellStyle.paddingBottom);
  const rowGap = Number.parseFloat(shellStyle.rowGap);
  const availableWidth = Math.max(1, Math.floor(viewportWidth - horizontalPadding));
  const availableHeight = Math.max(
    1,
    Math.floor(
      viewportHeight - verticalPadding - shellCopy.getBoundingClientRect().height - rowGap,
    ),
  );
  const width = Math.max(availableWidth, minimumWidth);
  const height = Math.max(availableHeight, minimumHeight);
  const scale = Math.min(1, availableWidth / width, availableHeight / height);

  canvas.style.width = `${width}px`;
  canvas.style.height = `${height}px`;
  canvas.style.transform = `scale(${scale})`;
  frame.style.width = `${width * scale}px`;
  frame.style.height = `${height * scale}px`;
};

const setStatus = (state, message) => {
  if (!status) {
    return;
  }
  status.dataset.state = state;
  status.textContent = message;
  requestAnimationFrame(layOutCanvas);
};

const showStartupError = (error) => {
  setStatus("error", "The game could not start. Reload the page or try a current browser.");
  console.error("Go Together startup failed:", error);
};

// F11 is the browser's, for its own fullscreen, and on a Mac so is
// Control-Command-F, macOS's own fullscreen shortcut, which Help names beside
// F11 because a Mac's default settings keep F11 itself. Stopped here, at the
// window before anything under it hears the key, neither reaches the canvas,
// where winit would cancel the browser's action for it as it does for every
// key, nor the start handler below, which cancels the first key's. Nothing
// cancels them, so the browser acts on them, and the game never sees them.
// Shift or Option with the chord make it another key, as they do in the desktop
// game.
const onAMac = /Mac/.test(navigator.platform || navigator.userAgent);
const isTheChord = (event) =>
  onAMac &&
  event.code === "KeyF" &&
  event.ctrlKey &&
  event.metaKey &&
  !event.shiftKey &&
  !event.altKey;
// As in the desktop game, the chord's F is the chord's until it is let go: its
// repeats and its release stay the browser's even once Control and Command are
// let go first, so a held chord never turns into an F that moves a player. Each
// fresh press of F decides afresh, so a release the page never hears - Safari
// sends none for a key let go while Command is held - holds nothing back.
let chordsF = false;
const isTheBrowsers = (event) => {
  if (event.key === "F11" || event.code === "F11") {
    return true;
  }
  if (event.code !== "KeyF") {
    return false;
  }
  if (event.type === "keyup") {
    const wasTheChords = chordsF;
    chordsF = false;
    return wasTheChords;
  }
  if (!event.repeat) {
    chordsF = isTheChord(event);
  }
  return chordsF || isTheChord(event);
};
for (const type of ["keydown", "keyup"]) {
  window.addEventListener(
    type,
    (event) => {
      if (isTheBrowsers(event)) {
        event.stopPropagation();
      }
    },
    { capture: true },
  );
}

if (canvas) {
  canvas.addEventListener(
    "pointerdown",
    () => {
      canvas.focus({ preventScroll: true });
    },
    { passive: true },
  );

  canvas.addEventListener("webglcontextlost", (event) => {
    event.preventDefault();
    setStatus("error", "Graphics were interrupted. Reload the page to continue.");
  });
}

layOutCanvas();
window.addEventListener("resize", layOutCanvas);
if (shellCopy && "ResizeObserver" in window) {
  new ResizeObserver(layOutCanvas).observe(shellCopy);
}

window.addEventListener("error", (event) => {
  showStartupError(event.error ?? event.message);
});

window.addEventListener("unhandledrejection", (event) => {
  showStartupError(event.reason);
});

try {
  if (!loader?.dataset.bindings || !loader.dataset.wasm || !canvas || !status) {
    throw new Error("The generated web loader is missing required page elements.");
  }

  const bindingsUrl = new URL(loader.dataset.bindings, document.baseURI).href;
  const wasmUrl = new URL(loader.dataset.wasm, document.baseURI).href;
  const bindings = await import(bindingsUrl);
  const response = await fetch(wasmUrl);
  if (!response.ok) {
    throw new Error(`WebAssembly download failed with HTTP ${response.status}.`);
  }
  const module = await WebAssembly.compile(await response.arrayBuffer());
  setStatus("waiting", "Click or press a key to start.");

  let started = false;
  const start = async (event) => {
    if (started) {
      return;
    }
    started = true;
    canvas.removeEventListener("pointerdown", start);
    canvas.removeEventListener("keydown", start);
    if (event.type === "keydown") {
      event.preventDefault();
    }
    canvas.focus({ preventScroll: true });
    setStatus("loading", "Starting Go Together...");

    try {
      // Never `initSync`: Chromium refuses to instantiate a module over 8 MB
      // synchronously on the main thread. See "Starting in a browser" in
      // docs/DISTRIBUTION.md.
      const wasm = await bindings.default({ module_or_path: module });
      // The game opens its window at the canvas's size, which winit writes
      // back onto the canvas; laid out again here, the canvas keeps the
      // size this page gives it even if the game could not read it.
      layOutCanvas();
      window.dispatchEvent(new CustomEvent("TrunkApplicationStarted", { detail: { wasm } }));
      setStatus("ready", "Ready.");
      guidance.textContent =
        "Keyboard is ready. Browser gamepads work when a standard mapping is available.";
    } catch (error) {
      showStartupError(error);
    }
  };

  canvas.addEventListener("pointerdown", start);
  canvas.addEventListener("keydown", start);
} catch (error) {
  showStartupError(error);
}
