const loader = document.querySelector("#go-together-loader");
const canvas = document.querySelector("#go-together");
const frame = document.querySelector(".canvas-frame");
const shell = document.querySelector(".game-shell");
const shellCopy = document.querySelector(".shell-copy");
const status = document.querySelector("#web-status");
const guidance = document.querySelector("#focus-guidance");
const gameWidth = 1040;
const gameHeight = 900;

const updateCanvasScale = () => {
  if (!canvas || !frame || !shell || !shellCopy) {
    return;
  }
  const viewportWidth = document.documentElement.clientWidth;
  const viewportHeight = window.visualViewport?.height ?? document.documentElement.clientHeight;
  const shellStyle = getComputedStyle(shell);
  const horizontalPadding =
    Number.parseFloat(shellStyle.paddingLeft) + Number.parseFloat(shellStyle.paddingRight);
  const verticalPadding =
    Number.parseFloat(shellStyle.paddingTop) + Number.parseFloat(shellStyle.paddingBottom);
  const rowGap = Number.parseFloat(shellStyle.rowGap);
  const availableWidth = Math.max(1, viewportWidth - horizontalPadding);
  const availableHeight = Math.max(
    1,
    viewportHeight - verticalPadding - shellCopy.getBoundingClientRect().height - rowGap,
  );
  const scale = Math.min(1, availableWidth / gameWidth, availableHeight / gameHeight);

  frame.style.width = `${gameWidth * scale}px`;
  frame.style.height = `${gameHeight * scale}px`;
  canvas.style.transform = `scale(${scale})`;
};

const setStatus = (state, message) => {
  if (!status) {
    return;
  }
  status.dataset.state = state;
  status.textContent = message;
  requestAnimationFrame(updateCanvasScale);
};

const showStartupError = (error) => {
  setStatus("error", "The game could not start. Reload the page or try a current browser.");
  console.error("Go Together startup failed:", error);
};

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

updateCanvasScale();
window.addEventListener("resize", updateCanvasScale);
window.visualViewport?.addEventListener("resize", updateCanvasScale);
if (shellCopy && "ResizeObserver" in window) {
  new ResizeObserver(updateCanvasScale).observe(shellCopy);
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
