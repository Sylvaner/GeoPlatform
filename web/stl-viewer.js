import * as THREE from "/assets/vendor/three.module.js";

const MAX_TRIANGLES = 2_000_000;
const decoder = new TextDecoder("utf-8");
const numberFormat = new Intl.NumberFormat("fr-FR", { maximumFractionDigits: 2 });
const dialog = document.getElementById("stl-viewer-dialog");

const state = {
  renderer: null,
  scene: null,
  camera: null,
  model: null,
  theta: -0.75,
  phi: 0.95,
  distance: 7.5,
  dragging: false,
  pointer: [0, 0],
  requestId: 0,
};

const elements = {
  canvas: document.getElementById("stl-viewer-canvas"),
  stage: document.getElementById("stl-viewer-stage"),
  loading: document.getElementById("stl-viewer-loading"),
  error: document.getElementById("stl-viewer-error"),
  name: document.getElementById("stl-viewer-name"),
  stats: document.getElementById("stl-viewer-stats"),
  triangles: document.getElementById("stl-viewer-triangles"),
  dimensions: document.getElementById("stl-viewer-dimensions"),
  size: document.getElementById("stl-viewer-size"),
};

function initialize() {
  if (state.renderer || !elements.canvas) {
    return;
  }
  state.renderer = new THREE.WebGLRenderer({
    canvas: elements.canvas,
    antialias: true,
    alpha: false,
    powerPreference: "high-performance",
  });
  state.renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
  state.renderer.setClearColor(0x101c20, 1);
  state.scene = new THREE.Scene();
  state.scene.background = new THREE.Color(0x101c20);
  state.camera = new THREE.PerspectiveCamera(38, 1, 0.05, 100);
  state.camera.up.set(0, 0, 1);
  state.scene.add(new THREE.HemisphereLight(0xe4f0e2, 0x42534f, 2.2));
  const keyLight = new THREE.DirectionalLight(0xffffff, 3.2);
  keyLight.position.set(-5, -7, 10);
  state.scene.add(keyLight);
  const fillLight = new THREE.DirectionalLight(0x8bc4ce, 1.15);
  fillLight.position.set(6, 4, 3);
  state.scene.add(fillLight);
  const grid = new THREE.GridHelper(8, 16, 0x67807a, 0x405651);
  grid.rotation.x = Math.PI / 2;
  grid.position.z = -2.05;
  state.scene.add(grid);
  bindControls();
  new ResizeObserver(render).observe(elements.stage);
}

function bindControls() {
  elements.canvas.addEventListener("pointerdown", (event) => {
    if (!state.model || event.button !== 0) {
      return;
    }
    state.dragging = true;
    state.pointer = [event.clientX, event.clientY];
    elements.canvas.setPointerCapture(event.pointerId);
  });
  elements.canvas.addEventListener("pointermove", (event) => {
    if (!state.dragging) {
      return;
    }
    const deltaX = event.clientX - state.pointer[0];
    const deltaY = event.clientY - state.pointer[1];
    state.pointer = [event.clientX, event.clientY];
    state.theta -= deltaX * 0.008;
    state.phi = THREE.MathUtils.clamp(state.phi + deltaY * 0.008, 0.12, 1.48);
    render();
  });
  for (const eventName of ["pointerup", "pointercancel", "lostpointercapture"]) {
    elements.canvas.addEventListener(eventName, () => {
      state.dragging = false;
    });
  }
  elements.canvas.addEventListener("wheel", (event) => {
    if (!state.model) {
      return;
    }
    event.preventDefault();
    state.distance = THREE.MathUtils.clamp(state.distance * Math.exp(event.deltaY * 0.001), 2.8, 22);
    render();
  }, { passive: false });
  document.getElementById("stl-viewer-close")?.addEventListener("click", () => dialog.close());
  document.getElementById("stl-viewer-reset")?.addEventListener("click", resetView);
  dialog.addEventListener("click", (event) => {
    if (event.target === dialog) {
      dialog.close();
    }
  });
  dialog.addEventListener("close", () => {
    state.requestId += 1;
    elements.loading.hidden = true;
    disposeModel();
  });
}

function setCamera() {
  const sinPhi = Math.sin(state.phi);
  state.camera.position.set(
    state.distance * sinPhi * Math.cos(state.theta),
    state.distance * sinPhi * Math.sin(state.theta),
    state.distance * Math.cos(state.phi),
  );
  state.camera.lookAt(0, 0, 0);
}

function render() {
  if (!state.renderer || !state.camera || !dialog.open) {
    return;
  }
  const width = Math.max(1, elements.stage.clientWidth);
  const height = Math.max(1, elements.stage.clientHeight);
  state.renderer.setSize(width, height, false);
  state.camera.aspect = width / height;
  state.camera.updateProjectionMatrix();
  setCamera();
  state.renderer.render(state.scene, state.camera);
}

function parseStl(buffer) {
  if (buffer.byteLength < 15) {
    throw new Error("Le fichier est trop petit pour être un STL valide.");
  }
  const bytes = new Uint8Array(buffer);
  const view = new DataView(buffer);
  const triangleCount = buffer.byteLength >= 84 ? view.getUint32(80, true) : 0;
  const expectedSize = 84 + triangleCount * 50;
  const header = decoder.decode(bytes.subarray(0, Math.min(bytes.length, 1024)));
  const asciiHint = /^\s*solid\b/i.test(header) && /\bfacet\s+normal\b/i.test(header);
  const isBinary = buffer.byteLength >= 84
    && (expectedSize === buffer.byteLength
      || (!asciiHint && triangleCount > 0 && expectedSize < buffer.byteLength));
  let positions;
  let format;

  if (isBinary) {
    if (triangleCount > MAX_TRIANGLES) {
      throw new Error("Le modèle dépasse la limite de 2 millions de triangles.");
    }
    if (expectedSize > buffer.byteLength) {
      throw new Error("Le fichier STL binaire est incomplet.");
    }
    positions = new Float32Array(triangleCount * 9);
    for (let triangle = 0; triangle < triangleCount; triangle += 1) {
      const start = 84 + triangle * 50 + 12;
      for (let vertex = 0; vertex < 3; vertex += 1) {
        for (let axis = 0; axis < 3; axis += 1) {
          const value = view.getFloat32(start + vertex * 12 + axis * 4, true);
          if (!Number.isFinite(value)) {
            throw new Error("Le STL contient des coordonnées invalides.");
          }
          positions[triangle * 9 + vertex * 3 + axis] = value;
        }
      }
    }
    format = "Binaire";
  } else {
    const source = decoder.decode(buffer);
    const number = "([+-]?(?:\\d+(?:\\.\\d*)?|\\.\\d+)(?:[eE][+-]?\\d+)?)";
    const vertexPattern = new RegExp(`\\bvertex\\s+${number}\\s+${number}\\s+${number}`, "gi");
    const values = [];
    let match;
    while ((match = vertexPattern.exec(source)) !== null) {
      for (let axis = 1; axis <= 3; axis += 1) {
        const value = Number(match[axis]);
        if (!Number.isFinite(value)) {
          throw new Error("Le STL contient des coordonnées invalides.");
        }
        values.push(value);
      }
      if (values.length > MAX_TRIANGLES * 9) {
        throw new Error("Le modèle dépasse la limite de 2 millions de triangles.");
      }
    }
    if (values.length < 9 || values.length % 9 !== 0) {
      throw new Error("Le fichier ne contient pas de triangles STL valides.");
    }
    positions = new Float32Array(values);
    format = "ASCII";
  }

  if (positions.length === 0) {
    throw new Error("Le STL ne contient aucun triangle.");
  }
  const min = [Infinity, Infinity, Infinity];
  const max = [-Infinity, -Infinity, -Infinity];
  for (let index = 0; index < positions.length; index += 3) {
    for (let axis = 0; axis < 3; axis += 1) {
      const value = positions[index + axis];
      min[axis] = Math.min(min[axis], value);
      max[axis] = Math.max(max[axis], value);
    }
  }
  const dimensions = min.map((value, axis) => max[axis] - value);
  const center = min.map((value, axis) => (value + max[axis]) / 2);
  const scale = 3.8 / Math.max(...dimensions, 1e-12);
  for (let index = 0; index < positions.length; index += 3) {
    positions[index] = (positions[index] - center[0]) * scale;
    positions[index + 1] = (positions[index + 1] - center[1]) * scale;
    positions[index + 2] = (positions[index + 2] - center[2]) * scale;
  }
  return { positions, dimensions, format, triangles: positions.length / 9 };
}

function disposeModel() {
  if (!state.model) {
    return;
  }
  state.scene.remove(state.model);
  state.model.geometry.dispose();
  state.model.material.dispose();
  state.model = null;
}

function resetView() {
  state.theta = -0.75;
  state.phi = 0.95;
  state.distance = 7.5;
  render();
}

function formatNumber(value) {
  return numberFormat.format(value);
}

function formatSize(bytes) {
  return bytes < 1024 * 1024
    ? `${formatNumber(bytes / 1024)} Ko`
    : `${formatNumber(bytes / (1024 * 1024))} Mo`;
}

async function open({ url, name }) {
  if (!dialog?.showModal) {
    throw new Error("La fenêtre de visualisation n’est pas disponible dans ce navigateur.");
  }
  initialize();
  const requestId = ++state.requestId;
  elements.name.textContent = name || "Fichier STL";
  elements.error.hidden = true;
  elements.stats.hidden = true;
  elements.loading.hidden = false;
  if (!dialog.open) {
    dialog.showModal();
  }
  try {
    const response = await fetch(url);
    if (!response.ok) {
      throw new Error(`Le serveur a répondu ${response.status}.`);
    }
    const buffer = await response.arrayBuffer();
    if (requestId !== state.requestId) {
      return;
    }
    const data = parseStl(buffer);
    disposeModel();
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute("position", new THREE.BufferAttribute(data.positions, 3));
    geometry.computeVertexNormals();
    geometry.computeBoundingSphere();
    const material = new THREE.MeshStandardMaterial({
      color: 0x64b5c3,
      roughness: 0.7,
      metalness: 0.08,
      side: THREE.DoubleSide,
    });
    state.model = new THREE.Mesh(geometry, material);
    state.scene.add(state.model);
    document.getElementById("stl-viewer-title").textContent = `Maquette STL · ${data.format}`;
    document.getElementById("stl-viewer-triangles").textContent = formatNumber(data.triangles);
    document.getElementById("stl-viewer-dimensions").textContent = data.dimensions.map(formatNumber).join(" × ");
    document.getElementById("stl-viewer-size").textContent = formatSize(buffer.byteLength);
    elements.stats.hidden = false;
    document.getElementById("stl-viewer-reset").disabled = false;
    resetView();
  } catch (error) {
    if (requestId === state.requestId) {
      elements.error.textContent = `Impossible de lire ce STL : ${error.message}`;
      elements.error.hidden = false;
      document.getElementById("stl-viewer-reset").disabled = true;
    }
  } finally {
    if (requestId === state.requestId) {
      elements.loading.hidden = true;
    }
  }
}

window.StlViewer = { open };