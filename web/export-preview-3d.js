import * as THREE from "/assets/vendor/three.module.js";

/**
 * Aperçu 3D léger du wizard d'export.
 * Le terrain est synthétique : il explique les réglages sans charger les lourdes données réelles.
 */
const ExportPreview3D = (() => {
  const state = {
    container: null,
    renderer: null,
    scene: null,
    camera: null,
    model: null,
    rotationY: -0.72,
    zoom: 13,
    dragging: false,
    pointer: [0, 0],
    config: null,
  };

  /** Initialise le moteur WebGL lorsque la cible du wizard existe. */
  function initialize() {
    state.container = document.getElementById("export-preview-3d");
    if (!state.container || state.renderer) {
      return;
    }
    try {
      state.renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
      state.renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 1.6));
      state.renderer.outputColorSpace = THREE.SRGBColorSpace;
      state.renderer.setClearColor(0x000000, 0);
      state.renderer.domElement.setAttribute("aria-label", "Aperçu 3D interactif de la maquette");
      state.container.append(state.renderer.domElement);
      state.scene = new THREE.Scene();
      state.camera = new THREE.PerspectiveCamera(34, 1, 0.1, 100);
      addLights();
      bindInteraction();
      new ResizeObserver(resize).observe(state.container);
      update(readInitialConfig());
    } catch (error) {
      showFallback(error);
    }
  }

  /** Ajoute un éclairage doux qui garde le relief lisible sur fond clair. */
  function addLights() {
    state.scene.add(new THREE.HemisphereLight(0xf5f8ef, 0x4c655d, 2.1));
    const sun = new THREE.DirectionalLight(0xffffff, 2.8);
    sun.position.set(7, 12, 8);
    state.scene.add(sun);
  }

  /** Relie rotation, zoom et redimensionnement au canvas uniquement. */
  function bindInteraction() {
    const canvas = state.renderer.domElement;
    canvas.addEventListener("pointerdown", startDrag);
    canvas.addEventListener("pointermove", drag);
    canvas.addEventListener("pointerup", stopDrag);
    canvas.addEventListener("pointercancel", stopDrag);
    canvas.addEventListener("wheel", zoom, { passive: false });
  }

  /** Commence une rotation et capture le pointeur pour une interaction continue. */
  function startDrag(event) {
    state.dragging = true;
    state.pointer = [event.clientX, event.clientY];
    event.currentTarget.setPointerCapture(event.pointerId);
  }

  /** Tourne le modèle en conservant une inclinaison confortable. */
  function drag(event) {
    if (!state.dragging) {
      return;
    }
    const deltaX = event.clientX - state.pointer[0];
    state.pointer = [event.clientX, event.clientY];
    state.rotationY += deltaX * 0.008;
    render();
  }

  /** Termine la rotation sans modifier l'orientation obtenue. */
  function stopDrag(event) {
    state.dragging = false;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  }

  /** Rapproche ou éloigne la caméra dans des limites adaptées à la maquette. */
  function zoom(event) {
    event.preventDefault();
    state.zoom = THREE.MathUtils.clamp(state.zoom + event.deltaY * 0.008, 8, 20);
    render();
  }

  /** Reconstruit la petite scène quand un réglage visible change. */
  function update(config = {}) {
    state.config = {
      roadWidth: Number(config.roadWidth) || 1,
      roadElevationM: Math.max(0, Number(config.roadElevationM) || 0),
      buildingScale: Number(config.buildingScale) || 1,
      treeScale: Number(config.treeScale) || 1,
      treeShape: config.treeShape || "deciduous",
      forestQuality: config.forestQuality || "standard",
      canopySmoothing: Number(config.canopySmoothing ?? 0.7),
      forestMaterial: config.forestMaterial || "procedural",
      forestColorLow: config.forestColorLow || "#061f05",
      forestColorHigh: config.forestColorHigh || "#156b0f",
      carveForestRoads: config.carveForestRoads === true,
      keepIsolatedTrees: config.keepIsolatedTrees === true,
      quality: config.quality || "standard",
      format: config.format || "stl",
      layerHeightM: Number(config.layerHeightM) || 1,
      layers: Array.isArray(config.layers) ? config.layers : [],
      colors: config.colors || {},
      proceduralMaterials: config.proceduralMaterials !== false,
    };
    if (!state.renderer) {
      return;
    }
    disposeModel();
    state.model = buildModel(state.config);
    state.scene.add(state.model);
    render();
  }

  /** Crée un terrain, des bâtiments, une route et des arbres synthétiques. */
  function buildModel(config) {
    const model = new THREE.Group();
    model.add(buildBase());
    model.add(config.format === "stl_layers"
      ? buildLayeredTerrain(config.quality, config.colors.ground, config.layerHeightM)
      : buildTerrain(config.quality, config.colors.ground));
    const enabled = new Set(config.layers);
    if (!enabled.size || enabled.has("buildings")) {
      addBuildings(
        model,
        config.buildingScale,
        config.colors.walls,
        config.colors.roofs,
        config.proceduralMaterials,
      );
    }
    if (!enabled.size || enabled.has("transport")) {
      model.add(buildRoad(config.roadWidth, config.colors.roads, config.roadElevationM));
    }
    if (!enabled.size || enabled.has("hydrography")) {
      model.add(buildWater(config.colors.water));
    }
    if (!enabled.size || enabled.has("lidar")) {
      if (config.treeShape === "canopy") {
        addForestCanopy(
          model,
          config.colors.trees,
          config.proceduralMaterials,
          config.forestQuality,
          config.canopySmoothing,
          config.forestMaterial,
          config.forestColorLow,
          config.forestColorHigh,
          config.carveForestRoads,
        );
        if (config.keepIsolatedTrees) {
          addIsolatedTrees(
            model,
            config.treeScale,
            config.colors.trees,
            config.proceduralMaterials,
          );
        }
      } else {
        addTrees(
          model,
          config.treeScale,
          config.treeShape,
          config.colors.trees,
          config.proceduralMaterials,
        );
      }
    }
    return model;
  }

  /** Produit le socle fermé représentatif d'un STL imprimable. */
  function buildBase() {
    const geometry = new THREE.BoxGeometry(10.2, 0.55, 10.2);
    const material = new THREE.MeshStandardMaterial({ color: 0x355f53, roughness: 0.82 });
    const mesh = new THREE.Mesh(geometry, material);
    mesh.position.y = -0.38;
    return mesh;
  }

  /** Produit un relief dont la finesse visible suit le niveau de qualité choisi. */
  function buildTerrain(quality, color) {
    const segments = { draft: 10, standard: 22, high: 38 }[quality] || 22;
    const geometry = new THREE.PlaneGeometry(10, 10, segments, segments);
    geometry.rotateX(-Math.PI / 2);
    const positions = geometry.attributes.position;
    for (let index = 0; index < positions.count; index += 1) {
      const x = positions.getX(index);
      const z = positions.getZ(index);
      positions.setY(index, terrainHeight(x, z));
    }
    geometry.computeVertexNormals();
    const material = new THREE.MeshStandardMaterial({
      color: color || 0x8dad88,
      roughness: 0.9,
      flatShading: quality !== "high",
    });
    return new THREE.Mesh(geometry, material);
  }

  /** Construit de vraies marches topographiques pour expliquer le STL par couches. */
  function buildLayeredTerrain(quality, color, layerHeightM) {
    const cells = { draft: 14, standard: 24, high: 34 }[quality] || 24;
    const step = 10 / cells;
    const stratum = THREE.MathUtils.clamp(layerHeightM * 0.11, 0.045, 0.55);
    const positions = [];
    const indices = [];
    const levels = Array.from({ length: cells }, (_, row) =>
      Array.from({ length: cells }, (_, column) => {
        const x = -5 + (column + 0.5) * step;
        const z = -5 + (row + 0.5) * step;
        return Math.floor(terrainHeight(x, z) / stratum) * stratum;
      }),
    );
    const quad = (a, b, c, d) => {
      const start = positions.length / 3;
      positions.push(...a, ...b, ...c, ...d);
      indices.push(start, start + 2, start + 1, start + 1, start + 2, start + 3);
    };
    for (let row = 0; row < cells; row += 1) {
      for (let column = 0; column < cells; column += 1) {
        const x0 = -5 + column * step;
        const x1 = x0 + step;
        const z0 = -5 + row * step;
        const z1 = z0 + step;
        const height = levels[row][column];
        quad([x0, height, z0], [x1, height, z0], [x0, height, z1], [x1, height, z1]);
        const east = levels[row]?.[column + 1];
        if (east !== undefined && east !== height) {
          const low = Math.min(height, east);
          const high = Math.max(height, east);
          quad([x1, high, z0], [x1, high, z1], [x1, low, z0], [x1, low, z1]);
        }
        const north = levels[row + 1]?.[column];
        if (north !== undefined && north !== height) {
          const low = Math.min(height, north);
          const high = Math.max(height, north);
          quad([x0, high, z1], [x1, high, z1], [x0, low, z1], [x1, low, z1]);
        }
      }
    }
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
    geometry.setIndex(indices);
    geometry.computeVertexNormals();
    return new THREE.Mesh(
      geometry,
      new THREE.MeshStandardMaterial({
        color: color || 0x8dad88,
        roughness: 0.92,
        flatShading: true,
        side: THREE.DoubleSide,
      }),
    );
  }

  /** Calcule la même hauteur synthétique pour tous les objets de la scène. */
  function terrainHeight(x, z) {
    return 0.42 + Math.sin(x * 0.7) * 0.38 + Math.cos(z * 0.58) * 0.27 + x * 0.035;
  }

  /** Ajoute quelques volumes urbains dont l'échelle verticale reste immédiatement visible. */
  function addBuildings(model, scale, wallColor, roofColor, procedural) {
    const positions = [
      [-2.7, -1.4, 1.4],
      [-1.2, -1.7, 2.1],
      [0.4, -1.25, 1.65],
      [2.2, -0.9, 2.45],
      [2.8, 1.25, 1.3],
      [-0.2, 1.55, 1.1],
    ];
    for (const [x, z, baseHeight] of positions) {
      const height = baseHeight * scale;
      const geometry = new THREE.BoxGeometry(0.9, height, 0.82);
      const material = variedMaterial(wallColor, 0xe8dfc9, 0.86, procedural, x * 17 + z);
      const building = new THREE.Mesh(geometry, material);
      building.position.set(x, terrainHeight(x, z) + height / 2, z);
      model.add(building);
      const roof = new THREE.Mesh(
        new THREE.ConeGeometry(0.68, 0.46, 4),
        variedMaterial(roofColor, 0x9a7448, 0.9, procedural, x * 31 - z),
      );
      roof.rotation.y = Math.PI / 4;
      roof.position.set(x, terrainHeight(x, z) + height + 0.23, z);
      model.add(roof);
    }
  }

  /** Crée une chaussée plate qui suit le relief et illustre sa largeur imprimée. */
  function buildRoad(width, color, elevationM) {
    const points = [
      new THREE.Vector3(-5.2, 0, 2.8),
      new THREE.Vector3(-2.2, 0, 1.7),
      new THREE.Vector3(0.6, 0, 2.2),
      new THREE.Vector3(2.8, 0, 0.7),
      new THREE.Vector3(5.2, 0, 0.2),
    ];
    const previewElevation = THREE.MathUtils.clamp(Number(elevationM) || 0, 0, 5) * 0.08;
    return buildTerrainRibbon(points, 0.08 + width * 0.075, 0.075 + previewElevation, {
      color: color || 0x1b1f1d,
      roughness: 0.95,
    });
  }

  /** Crée un ruban plat dont chaque bord repose sur le relief synthétique. */
  function buildTerrainRibbon(points, halfWidth, offset, materialOptions) {
    const curve = new THREE.CatmullRomCurve3(points);
    const vertices = [];
    const indices = [];
    const segments = 48;
    for (let index = 0; index <= segments; index += 1) {
      const position = curve.getPoint(index / segments);
      const tangent = curve.getTangent(index / segments).normalize();
      const normal = new THREE.Vector3(-tangent.z, 0, tangent.x).normalize();
      const left = position.clone().addScaledVector(normal, halfWidth);
      const right = position.clone().addScaledVector(normal, -halfWidth);
      left.y = terrainHeight(left.x, left.z) + offset;
      right.y = terrainHeight(right.x, right.z) + offset;
      vertices.push(left, right);
      if (index < segments) {
        const first = index * 2;
        indices.push(first, first + 2, first + 1, first + 1, first + 2, first + 3);
      }
    }
    const geometry = new THREE.BufferGeometry().setFromPoints(vertices);
    geometry.setIndex(indices);
    geometry.computeVertexNormals();
    const material = new THREE.MeshStandardMaterial({
      ...materialOptions,
      side: THREE.DoubleSide,
    });
    return new THREE.Mesh(geometry, material);
  }

  /** Ajoute une eau bleue qui épouse le relief au lieu de flotter à une altitude fixe. */
  function buildWater(color) {
    const points = [
      new THREE.Vector3(-5.1, 0, -0.55),
      new THREE.Vector3(-3.0, 0, -0.95),
      new THREE.Vector3(-0.8, 0, -0.45),
      new THREE.Vector3(1.2, 0, -1.0),
      new THREE.Vector3(3.0, 0, -0.55),
      new THREE.Vector3(5.1, 0, -0.95),
    ];
    return buildTerrainRibbon(points, 0.24, 0.055, {
      color: color || 0x2f8fbd,
      roughness: 0.28,
      metalness: 0.06,
      transparent: true,
      opacity: 0.88,
    });
  }

  /** Répartit des arbres simples sur les zones libres de la maquette. */
  function addTrees(model, scale, shape, color, procedural) {
    const positions = [
      [-3.8, -3.1, 0.8], [-2.4, 3.2, 1], [-0.9, 3.55, 0.75], [0.9, 3.3, 1.15],
      [2.5, 3.65, 0.9], [3.8, 2.5, 1.2], [3.65, -2.4, 0.8], [-4.0, 0.5, 1.1],
    ];
    for (const [x, z, variation] of positions) {
      const tree = buildTree(1.45 * scale * variation, shape, color, procedural, x * 13 + z);
      tree.position.set(x, terrainHeight(x, z), z);
      model.add(tree);
    }
  }

  /** Montre quelques arbres ponctuels hors de la nappe forestière continue. */
  function addIsolatedTrees(model, scale, color, procedural) {
    const positions = [[-4.7, 4.45, 0.8], [4.75, -4.2, 1.05], [0.2, -4.55, 0.9]];
    for (const [x, z, variation] of positions) {
      const tree = buildTree(1.35 * scale * variation, "deciduous", color, procedural, x * 17 + z);
      tree.position.set(x, terrainHeight(x, z), z);
      model.add(tree);
    }
  }

  /** Représente la végétation LiDAR comme une nappe irrégulière qui suit le relief. */
  function addForestCanopy(
    model,
    color,
    procedural,
    quality,
    smoothing,
    materialMode,
    lowColor,
    highColor,
    carveRoads,
  ) {
    const smooth = THREE.MathUtils.clamp(Number(smoothing), 0, 1);
    const contour = smoothForestContour(quality, smooth);
    const holes = carveRoads ? [forestRoadOpening()] : [];
    const faces = THREE.ShapeUtils.triangulateShape(contour, holes);
    const points = contour.concat(...holes);
    const positions = [];
    const indices = [];
    for (const point of points) {
      positions.push(point.x, forestPreviewHeight(point.x, point.y, smooth), point.y);
    }
    for (const face of faces) {
      indices.push(face[0], face[1], face[2]);
    }
    addSmoothCanopyWall(positions, indices, contour, smooth, false);
    for (const hole of holes) addSmoothCanopyWall(positions, indices, hole, smooth, true);
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
    geometry.setIndex(indices);
    const uv = [];
    for (let index = 0; index < positions.length; index += 3) {
      uv.push(positions[index] / 10 + 0.5, positions[index + 2] / 10 + 0.5);
    }
    geometry.setAttribute("uv", new THREE.Float32BufferAttribute(uv, 2));
    geometry.computeVertexNormals();
    const material = new THREE.MeshStandardMaterial({
      color: materialMode === "texture" ? (color || 0x3f7856) : 0xffffff,
      roughness: procedural ? 0.88 : 0.96,
      flatShading: false,
      side: THREE.DoubleSide,
    });
    if (materialMode === "procedural") {
      material.map = forestRampTexture(lowColor, highColor);
    }
    model.add(new THREE.Mesh(geometry, material));
  }

  /** Interpole une lisière fermée entre les points témoins au lieu d'afficher les cellules. */
  function smoothForestContour(quality, smoothing) {
    const anchors = [
      [-4.75, -2.75], [-3.15, -3.65], [-1.2, -3.35], [0.45, -2.55],
      [2.2, -3.5], [4.35, -2.55], [4.65, -0.65], [3.65, 0.75],
      [4.25, 2.65], [2.45, 3.8], [0.55, 3.35], [-1.2, 4.1],
      [-3.3, 3.45], [-4.55, 1.8], [-4.15, 0.15],
    ].map(([x, z]) => new THREE.Vector3(x, 0, z));
    const segments = { draft: 28, standard: 44, high: 68 }[quality] || 44;
    const curve = new THREE.CatmullRomCurve3(anchors, true, "centripetal");
    const curved = curve.getSpacedPoints(segments).slice(0, -1);
    return curved.map((point, index) => {
      const position = index * anchors.length / curved.length;
      const first = Math.floor(position) % anchors.length;
      const second = (first + 1) % anchors.length;
      const ratio = position - Math.floor(position);
      const linear = anchors[first].clone().lerp(anchors[second], ratio);
      return new THREE.Vector2(
        THREE.MathUtils.lerp(linear.x, point.x, smoothing),
        THREE.MathUtils.lerp(linear.z, point.z, smoothing),
      );
    });
  }

  /** Crée une ouverture courbe et légère autour de la route dans la forêt témoin. */
  function forestRoadOpening() {
    const path = new THREE.CatmullRomCurve3([
      new THREE.Vector3(-4.0, 0, 2.35),
      new THREE.Vector3(-2.2, 0, 1.7),
      new THREE.Vector3(0.5, 0, 2.15),
      new THREE.Vector3(2.8, 0, 0.72),
      new THREE.Vector3(4.0, 0, 0.35),
    ]);
    const left = [];
    const right = [];
    for (let index = 0; index <= 24; index += 1) {
      const ratio = index / 24;
      const point = path.getPoint(ratio);
      const tangent = path.getTangent(ratio).normalize();
      const normal = new THREE.Vector3(-tangent.z, 0, tangent.x).normalize();
      left.push(new THREE.Vector2(point.x + normal.x * 0.22, point.z + normal.z * 0.22));
      right.push(new THREE.Vector2(point.x - normal.x * 0.22, point.z - normal.z * 0.22));
    }
    return left.concat(right.reverse());
  }

  /** Calcule une hauteur continue et moins agitée lorsque le lissage augmente. */
  function forestPreviewHeight(x, z, smoothing) {
    const wave = 0.62
      + Math.sin(x * 1.8 + z * 0.45) * (0.20 - smoothing * 0.09)
      + Math.cos(z * 2.1 - x) * (0.15 - smoothing * 0.07);
    return terrainHeight(x, z) + wave;
  }

  /** Ferme le pourtour courbe de la canopée jusqu'au relief pour garder un volume lisible. */
  function addSmoothCanopyWall(positions, indices, contour, smoothing, invert) {
    for (let index = 0; index < contour.length; index += 1) {
      const first = contour[index];
      const second = contour[(index + 1) % contour.length];
      const start = positions.length / 3;
      positions.push(
        first.x, forestPreviewHeight(first.x, first.y, smoothing), first.y,
        second.x, forestPreviewHeight(second.x, second.y, smoothing), second.y,
        first.x, terrainHeight(first.x, first.y) + 0.04, first.y,
        second.x, terrainHeight(second.x, second.y) + 0.04, second.y,
      );
      if (invert) indices.push(start, start + 2, start + 1, start + 1, start + 2, start + 3);
      else indices.push(start, start + 1, start + 2, start + 1, start + 3, start + 2);
    }
  }

  /** Génère un aperçu léger de la rampe Noise → Color Ramp utilisée dans Blender. */
  function forestRampTexture(lowColor, highColor) {
    const size = 48;
    const canvas = document.createElement("canvas");
    canvas.width = size;
    canvas.height = size;
    const context = canvas.getContext("2d");
    const image = context.createImageData(size, size);
    const low = new THREE.Color(lowColor || "#061f05");
    const high = new THREE.Color(highColor || "#156b0f");
    for (let index = 0; index < size * size; index += 1) {
      const x = index % size;
      const y = Math.floor(index / size);
      const noise = THREE.MathUtils.clamp(
        0.5 + Math.sin(x * 0.71 + y * 0.37) * 0.25 + Math.cos(y * 1.17 - x * 0.19) * 0.2,
        0,
        1,
      );
      const color = low.clone().lerp(high, noise);
      image.data[index * 4] = Math.round(color.r * 255);
      image.data[index * 4 + 1] = Math.round(color.g * 255);
      image.data[index * 4 + 2] = Math.round(color.b * 255);
      image.data[index * 4 + 3] = 255;
    }
    context.putImageData(image, 0, 0);
    const texture = new THREE.CanvasTexture(canvas);
    texture.colorSpace = THREE.SRGBColorSpace;
    texture.wrapS = THREE.RepeatWrapping;
    texture.wrapT = THREE.RepeatWrapping;
    texture.repeat.set(5, 5);
    return texture;
  }

  /** Dessine une lisière non géométrique afin d'éviter l'effet de disque ou de quadrillage. */
  function canopyCell(column, row, cells, carveRoads) {
    const x = -5 + ((column + 0.5) * 10) / cells;
    const z = -5 + ((row + 0.5) * 10) / cells;
    const firstMass = ((x + 1.25) / 4.25) ** 2 + ((z - 1.65) / 3.05) ** 2;
    const secondMass = ((x - 3.15) / 2.1) ** 2 + ((z + 2.45) / 1.65) ** 2;
    const edgeNoise = Math.sin(x * 2.4 + z * 1.1) * 0.12 + Math.cos(z * 3.1 - x) * 0.08;
    const clearing = ((x + 0.2) / 1.25) ** 2 + ((z - 1.2) / 0.72) ** 2 < 1;
    const road = [
      [-5.2, 2.8],
      [-2.2, 1.7],
      [0.6, 2.2],
      [2.8, 0.7],
      [5.2, 0.2],
    ];
    const crossesRoad = carveRoads && road.some((start, index) => {
      const end = road[index + 1];
      return end && pointSegmentDistance2D([x, z], start, end) < 0.32;
    });
    return (firstMass < 1 + edgeNoise || secondMass < 1 + edgeNoise)
      && !clearing
      && !crossesRoad;
  }

  /** Distance plane utilisée uniquement pour prévisualiser le corridor routier. */
  function pointSegmentDistance2D(point, start, end) {
    const dx = end[0] - start[0];
    const dy = end[1] - start[1];
    const lengthSquared = dx * dx + dy * dy;
    const ratio = lengthSquared
      ? THREE.MathUtils.clamp(
        ((point[0] - start[0]) * dx + (point[1] - start[1]) * dy) / lengthSquared,
        0,
        1,
      )
      : 0;
    const nearestX = start[0] + dx * ratio;
    const nearestY = start[1] + dy * ratio;
    return Math.hypot(point[0] - nearestX, point[1] - nearestY);
  }

  /** Ferme verticalement une lisière entre la canopée et le terrain synthétique. */
  function addCanopyWall(positions, indices, ax, az, bx, bz, step) {
    const x1 = -5 + ax * step;
    const z1 = -5 + az * step;
    const x2 = -5 + bx * step;
    const z2 = -5 + bz * step;
    const canopy = (x, z) => terrainHeight(x, z) + 0.62 + Math.sin(x * 1.8 + z * 0.45) * 0.14 + Math.cos(z * 2.1 - x) * 0.1;
    const start = positions.length / 3;
    positions.push(
      x1, canopy(x1, z1), z1,
      x2, canopy(x2, z2), z2,
      x1, terrainHeight(x1, z1) + 0.04, z1,
      x2, terrainHeight(x2, z2) + 0.04, z2,
    );
    indices.push(start, start + 2, start + 1, start + 1, start + 2, start + 3);
  }

  /** Produit un volume fermé avec des pentes ascendantes, adapté à l'impression sans support. */
  function buildTree(height, shape, color, procedural, seed) {
    const tree = new THREE.Group();
    const trunkHeight = Math.max(0.45, height * 0.34);
    const trunk = new THREE.Mesh(
      new THREE.CylinderGeometry(0.09, 0.13, trunkHeight, 8),
      new THREE.MeshStandardMaterial({ color: 0x614630, roughness: 1 }),
    );
    trunk.position.y = trunkHeight / 2;
    tree.add(trunk);
    const foliageMaterial = variedMaterial(color, 0x3f7856, 0.9, procedural, seed);
    if (shape === "conifer") {
      for (let level = 0; level < 3; level += 1) {
        const coneHeight = height * 0.52;
        const cone = new THREE.Mesh(
          new THREE.ConeGeometry(0.52 - level * 0.08, coneHeight, 12),
          foliageMaterial,
        );
        cone.position.y = trunkHeight + level * height * 0.2 + coneHeight / 2 - 0.1;
        tree.add(cone);
      }
    } else {
      const profile = [
        new THREE.Vector2(0.02, 0),
        new THREE.Vector2(0.34, height * 0.18),
        new THREE.Vector2(0.56, height * 0.42),
        new THREE.Vector2(0.46, height * 0.68),
        new THREE.Vector2(0.18, height * 0.84),
        new THREE.Vector2(0.01, height),
      ];
      const crown = new THREE.Mesh(new THREE.LatheGeometry(profile, 14), foliageMaterial);
      crown.position.y = trunkHeight * 0.72;
      tree.add(crown);
    }
    return tree;
  }

  /** Produit une matière locale légèrement nuancée, sans image ni requête réseau. */
  function variedMaterial(color, fallback, roughness, procedural, seed) {
    const resolved = color || fallback;
    const material = new THREE.MeshStandardMaterial({ color: resolved, roughness });
    if (!procedural) {
      return material;
    }
    material.map = proceduralTexture(resolved, seed);
    material.color.set(0xffffff);
    return material;
  }

  /** Génère un petit motif déterministe réutilisable par Three.js comme texture. */
  function proceduralTexture(color, seed) {
    const size = 24;
    const canvas = document.createElement("canvas");
    canvas.width = size;
    canvas.height = size;
    const context = canvas.getContext("2d");
    const pixels = context.createImageData(size, size);
    const base = new THREE.Color(color);
    for (let index = 0; index < size * size; index += 1) {
      const x = index % size;
      const y = Math.floor(index / size);
      const wave = Math.sin((x + seed) * 1.73 + (y - seed) * 2.31) * 0.055;
      const grain = Math.sin((x * 7 + y * 13 + seed) * 0.37) * 0.025;
      const factor = 1 + wave + grain;
      pixels.data[index * 4] = Math.round(THREE.MathUtils.clamp(base.r * factor, 0, 1) * 255);
      pixels.data[index * 4 + 1] = Math.round(THREE.MathUtils.clamp(base.g * factor, 0, 1) * 255);
      pixels.data[index * 4 + 2] = Math.round(THREE.MathUtils.clamp(base.b * factor, 0, 1) * 255);
      pixels.data[index * 4 + 3] = 255;
    }
    context.putImageData(pixels, 0, 0);
    const texture = new THREE.CanvasTexture(canvas);
    texture.colorSpace = THREE.SRGBColorSpace;
    texture.wrapS = THREE.RepeatWrapping;
    texture.wrapT = THREE.RepeatWrapping;
    texture.repeat.set(2, 2);
    return texture;
  }

  /** Libère les géométries et matériaux avant de reconstruire l'aperçu. */
  function disposeModel() {
    if (!state.model) {
      return;
    }
    state.scene.remove(state.model);
    state.model.traverse((object) => {
      object.geometry?.dispose();
      if (Array.isArray(object.material)) {
        object.material.forEach(disposeMaterial);
      } else {
        disposeMaterial(object.material);
      }
    });
  }

  /** Libère également la texture procédurale attachée à une matière. */
  function disposeMaterial(material) {
    material?.map?.dispose();
    material?.dispose();
  }

  /** Adapte le canvas à la place réellement disponible dans le panneau. */
  function resize() {
    if (!state.renderer || !state.container) {
      return;
    }
    const width = Math.max(1, state.container.clientWidth);
    const height = Math.max(1, state.container.clientHeight);
    state.renderer.setSize(width, height, false);
    state.camera.aspect = width / height;
    state.camera.updateProjectionMatrix();
    render();
  }

  /** Place la caméra et dessine une image uniquement lorsqu'elle est utile. */
  function render() {
    if (!state.renderer || !state.model) {
      return;
    }
    state.model.rotation.x = 0;
    state.model.rotation.y = state.rotationY;
    state.camera.position.set(0, 8.2, state.zoom);
    state.camera.lookAt(0, 0.45, 0);
    state.renderer.render(state.scene, state.camera);
  }

  /** Relit les contrôles déjà présents lorsque le module se charge après le wizard. */
  function readInitialConfig() {
    const selected = (name, fallback) =>
      document.querySelector(`input[name="${name}"]:checked`)?.value || fallback;
    return {
      roadWidth: document.getElementById("export-road-width")?.value,
      roadElevationM: document.getElementById("export-road-elevation")?.value,
      buildingScale: document.getElementById("export-building-scale")?.value,
      treeScale: document.getElementById("export-tree-scale")?.value,
      treeShape: selected("export-tree-shape", "deciduous"),
      forestQuality: selected("export-forest-quality", "standard"),
      canopySmoothing: document.getElementById("export-canopy-smoothing")?.value,
      forestMaterial: selected("export-forest-material", "procedural"),
      forestColorLow: document.getElementById("export-forest-low-color")?.value,
      forestColorHigh: document.getElementById("export-forest-high-color")?.value,
      carveForestRoads: document.getElementById("export-carve-forest-roads")?.checked,
      keepIsolatedTrees: document.getElementById("export-keep-isolated-trees")?.checked,
      quality: (() => {
        const grid = Number(document.getElementById("export-terrain-grid")?.value) || 50;
        return grid <= 10 ? "high" : grid <= 40 ? "standard" : "draft";
      })(),
      format: selected("export-format", "stl"),
      layerHeightM: document.getElementById("export-layer-height")?.value,
      layers: [...document.querySelectorAll('input[name="export-layers"]:checked')].map(
        (input) => input.value,
      ),
      proceduralMaterials: document.getElementById("export-procedural-materials")?.checked !== false,
    };
  }

  /** Affiche un repli explicite si WebGL n'est pas disponible sur le poste. */
  function showFallback(error) {
    state.container?.classList.add("is-unavailable");
    if (state.container) {
      state.container.textContent = "Aperçu 3D indisponible sur ce navigateur.";
    }
    console.warn("Aperçu 3D indisponible", error);
  }

  return { initialize, update };
})();

window.ExportPreview3D = ExportPreview3D;
window.addEventListener("geo-export-preview", (event) => ExportPreview3D.update(event.detail));
document.addEventListener("DOMContentLoaded", ExportPreview3D.initialize);
