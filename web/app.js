"use strict";

const FRANCE_VIEW = { center: [46.55, 2.25], zoom: 6 };
const FRANCE_BOUNDS = [
  [41.2, -5.5],
  [51.5, 9.7],
];
const REFRESH_INTERVAL_MS = 8000;
const STATUS_LABELS = {
  queued: "En attente",
  running: "En cours",
  completed: "Terminé",
  failed: "Échec",
  cancelled: "Annulé",
};
const LAYER_SYMBOLS = {
  elevation: "ALT",
  orthophoto: "ORT",
  lidar: "LID",
  buildings: "BAT",
  transport: "TRA",
  cadastre: "CAD",
  hydrography: "EAU",
};

const state = {
  map: null,
  coverageLayer: null,
  selectionLayer: null,
  selectionMarker: null,
  selectingCenter: false,
  coverageFeatures: [],
  coverageSignature: null,
  layerCatalog: [],
  jobs: [],
  serverLogs: [],
  panel: null,
  selection: { latitude: FRANCE_VIEW.center[0], longitude: FRANCE_VIEW.center[1], sizeM: 3000 },
};

const elements = {};

/** Initialise la carte, les événements et le premier chargement de données. */
async function initialize() {
  cacheElements();
  initializeMap();
  bindEvents();
  initializeExportWizard();
  setSelection(FRANCE_VIEW.center[0], FRANCE_VIEW.center[1]);
  await refreshAll();
  window.setInterval(() => {
    if (!document.hidden) {
      refreshLiveData();
    }
  }, REFRESH_INTERVAL_MS);
}

/** Mémorise les éléments utilisés plusieurs fois afin de garder les fonctions lisibles. */
function cacheElements() {
  for (const id of [
    "connection-status",
    "open-download",
    "export-show-wizard",
    "empty-download",
    "refresh-data",
    "zones-count",
    "storage-size",
    "active-jobs",
    "view-france",
    "fit-zones",
    "empty-state",
    "selection-guide",
    "cancel-selection",
    "panel-backdrop",
    "download-panel",
    "export-panel",
    "activity-panel",
    "download-form",
    "zone-name",
    "latitude",
    "longitude",
    "use-map-center",
    "layers-list",
    "select-all-layers",
    "selected-layer-count",
    "submit-download",
    "activity-badge",
    "activity-summary",
    "job-list",
    "server-log-list",
    "toast-region",
  ]) {
    elements[toCamelCase(id)] = document.getElementById(id);
  }
}

/** Branche le module d'export sur les seules données dont il a besoin. */
function initializeExportWizard() {
  window.ExportWizard?.initialize({
    fetchJson,
    getSources: () => state.coverageFeatures,
    getCurrentSelection: () => ({
      area: {
        center: [state.selection.latitude, state.selection.longitude],
        size_m: state.selection.sizeM,
      },
    }),
    getLayers: () => state.layerCatalog,
    showToast,
    closePanel: closePanels,
    refreshData: refreshLiveData,
  });
}

/** Convertit un identifiant HTML avec tirets en nom de propriété JavaScript. */
function toCamelCase(value) {
  return value.replace(/-([a-z])/g, (_, letter) => letter.toUpperCase());
}

/** Prépare une carte France bornée et une couche de tuiles volontairement modérée. */
function initializeMap() {
  if (!window.L) {
    showToast("La bibliothèque cartographique n’a pas pu être chargée.", true);
    return;
  }
  state.map = L.map("map", {
    center: FRANCE_VIEW.center,
    zoom: FRANCE_VIEW.zoom,
    minZoom: 5,
    maxZoom: 19,
    maxBounds: [
      [40.5, -7.5],
      [52.5, 11.5],
    ],
    maxBoundsViscosity: 0.75,
    preferCanvas: true,
    zoomControl: false,
  });
  state.map.fitBounds(FRANCE_BOUNDS, { padding: [28, 28], animate: false });
  L.tileLayer("https://tile.openstreetmap.org/{z}/{x}/{y}.png", {
    attribution: '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a>',
    maxZoom: 19,
    updateWhenIdle: true,
    keepBuffer: 2,
  }).addTo(state.map);
  L.control.zoom({ position: "bottomright" }).addTo(state.map);
  state.coverageLayer = L.geoJSON(null, {
    style: coverageStyle,
    onEachFeature: bindCoveragePopup,
  }).addTo(state.map);
  state.map.on("click", (event) => {
    if (state.selectingCenter) {
      chooseCenter(event.latlng);
      return;
    }
    if (state.panel === "download") {
      setSelection(event.latlng.lat, event.latlng.lng);
      focusSelection();
    }
  });
  state.map.on("popupopen", (event) => {
    if (state.selectingCenter || state.panel === "download") {
      state.map.closePopup(event.popup);
    }
  });
}

/** Relie les boutons et champs aux interactions de la page. */
function bindEvents() {
  elements.openDownload.addEventListener("click", startZoneSelection);
  elements.exportShowWizard.addEventListener("click", () => openPanel("exports"));
  elements.emptyDownload.addEventListener("click", startZoneSelection);
  elements.cancelSelection.addEventListener("click", closePanels);
  elements.panelBackdrop.addEventListener("click", closePanels);
  document.querySelectorAll(".close-panel").forEach((button) => {
    button.addEventListener("click", closePanels);
  });
  document.querySelectorAll("[data-navigation]").forEach((button) => {
    button.addEventListener("click", () => navigate(button.dataset.navigation));
  });
  elements.refreshData.addEventListener("click", refreshAll);
  elements.viewFrance.addEventListener("click", () => {
    state.map?.fitBounds(FRANCE_BOUNDS, { padding: [28, 28] });
  });
  elements.fitZones.addEventListener("click", fitCoverage);
  elements.useMapCenter.addEventListener("click", startZoneSelection);
  for (const input of [elements.latitude, elements.longitude]) {
    input.addEventListener("change", updateSelectionFromInputs);
  }
  document.querySelectorAll('input[name="size"]').forEach((input) => {
    input.addEventListener("change", () => {
      state.selection.sizeM = Number(input.value);
      drawSelection();
      focusSelection();
    });
  });
  elements.selectAllLayers.addEventListener("change", toggleAllLayers);
  elements.downloadForm.addEventListener("submit", submitDownload);
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      closePanels();
    }
  });
}

/** Affiche la carte ou ouvre le panneau correspondant à un élément du menu. */
function navigate(destination) {
  if (destination === "activity") {
    openPanel("activity");
    return;
  }
  if (destination === "exports") {
    openPanel("exports");
    return;
  }
  closePanels();
}

/** Ouvre un panneau latéral et synchronise l'état visuel du menu. */
function openPanel(name) {
  closePanels(false);
  state.panel = name;
  const panel =
    name === "download"
      ? elements.downloadPanel
      : name === "exports"
        ? elements.exportPanel
        : elements.activityPanel;
  panel.classList.add("is-open");
  panel.setAttribute("aria-hidden", "false");
  elements.panelBackdrop.hidden = name === "download";
  document.body.classList.toggle("has-download-panel", name === "download");
  setActiveNavigation(["activity", "exports"].includes(name) ? name : "map");
  if (name === "download") {
    drawSelection();
  } else if (name === "exports") {
    window.ExportWizard?.open();
  }
  resizeMapAfterPanelChange(name === "download");
}

/** Ferme tous les panneaux et retire l'emprise de préparation si nécessaire. */
function closePanels(resetNavigation = true) {
  for (const panel of [elements.downloadPanel, elements.exportPanel, elements.activityPanel]) {
    panel.classList.remove("is-open");
    panel.setAttribute("aria-hidden", "true");
  }
  elements.panelBackdrop.hidden = true;
  elements.selectionGuide.hidden = true;
  document.body.classList.remove("has-download-panel");
  document.body.classList.remove("is-selecting-zone");
  state.map?.getContainer().classList.remove("is-selecting");
  state.selectingCenter = false;
  state.panel = null;
  window.ExportWizard?.close();
  if (state.selectionLayer && state.map) {
    state.map.removeLayer(state.selectionLayer);
    state.selectionLayer = null;
  }
  if (state.selectionMarker && state.map) {
    state.map.removeLayer(state.selectionMarker);
    state.selectionMarker = null;
  }
  if (resetNavigation) {
    setActiveNavigation("map");
  }
  resizeMapAfterPanelChange(false);
}

/** Passe en mode pointage afin que le prochain clic définisse le centre. */
function startZoneSelection() {
  const shouldResetName = state.panel !== "download";
  closePanels(false);
  if (shouldResetName) {
    elements.zoneName.value = "";
  }
  state.selectingCenter = true;
  elements.selectionGuide.hidden = false;
  document.body.classList.add("is-selecting-zone");
  state.map?.closePopup();
  state.map?.getContainer().classList.add("is-selecting");
  setActiveNavigation("map");
}

/** Valide le point cliqué avant d'ouvrir la configuration de la zone. */
function chooseCenter(position) {
  setSelection(position.lat, position.lng);
  state.selectingCenter = false;
  elements.selectionGuide.hidden = true;
  document.body.classList.remove("is-selecting-zone");
  state.map?.getContainer().classList.remove("is-selecting");
  openPanel("download");
}

/** Redimensionne Leaflet après l'animation du panneau puis cadre l'emprise choisie. */
function resizeMapAfterPanelChange(focusZone) {
  window.setTimeout(() => {
    state.map?.invalidateSize();
    if (focusZone) {
      focusSelection();
    }
  }, 240);
}

/** Marque l'élément courant du menu supérieur. */
function setActiveNavigation(destination) {
  document.querySelectorAll("[data-navigation]").forEach((button) => {
    button.classList.toggle("is-active", button.dataset.navigation === destination);
  });
}

/** Charge en parallèle les quatre réponses nécessaires au premier affichage. */
async function refreshAll() {
  setRefreshState(true);
  const results = await Promise.allSettled([
    loadLayers(),
    loadDashboard(),
    loadCoverage(),
    loadJobs(),
    loadServerLogs(),
  ]);
  const failed = results.find((result) => result.status === "rejected");
  updateConnection(!failed);
  if (failed) {
    showToast(normalizeError(failed.reason), true);
  }
  setRefreshState(false);
}

/** Actualise les données dynamiques sans recharger le catalogue des couches. */
async function refreshLiveData() {
  const results = await Promise.allSettled([
    loadDashboard(),
    loadCoverage(),
    loadJobs(),
    loadServerLogs(),
  ]);
  updateConnection(results.every((result) => result.status === "fulfilled"));
}

/** Charge les couches disponibles puis construit leurs cases à cocher. */
async function loadLayers() {
  const layers = await fetchJson("/api/v1/layers");
  state.layerCatalog = layers;
  renderLayerOptions(layers);
  state.coverageLayer?.eachLayer((layer) => {
    layer.setStyle?.(coverageStyle(layer.feature));
  });
  window.ExportWizard?.refreshSources();
}

/** Charge les compteurs globaux et l'occupation du disque. */
async function loadDashboard() {
  const dashboard = await fetchJson("/api/v1/dashboard");
  elements.storageSize.textContent = formatBytes(dashboard.storage.total_bytes);
  const active = dashboard.jobs.queued + dashboard.jobs.running;
  elements.activeJobs.textContent = active ? String(active) : "Aucun";
  renderActivitySummary(dashboard.jobs);
  elements.activityBadge.hidden = active === 0;
  elements.activityBadge.textContent = active > 99 ? "99+" : String(active);
}

/** Charge les dernières emprises seulement, puis les dessine avec dédoublonnage. */
async function loadCoverage() {
  const collection = await fetchJson("/api/v1/map?kind=download&limit=500");
  const features = deduplicateCoverage(collection.features || []).filter((feature) =>
    ["completed", "running", "queued"].includes(feature.properties?.status),
  );
  const signature = JSON.stringify(features);
  state.coverageFeatures = features;
  if (state.coverageLayer && signature !== state.coverageSignature) {
    state.coverageLayer.clearLayers();
    state.coverageLayer.addData({ type: "FeatureCollection", features });
    state.coverageSignature = signature;
  }
  elements.zonesCount.textContent = String(features.length);
  elements.emptyState.hidden = features.length !== 0;
  window.ExportWizard?.refreshSources();
}

/** Garde une seule représentation lorsqu'un cache hit référence la même emprise. */
function deduplicateCoverage(features) {
  const unique = new Map();
  for (const feature of features) {
    const key = JSON.stringify([
      feature.geometry?.coordinates,
      [...(feature.properties?.layers || [])].sort(),
    ]);
    if (!unique.has(key)) {
      unique.set(key, feature);
    }
  }
  return [...unique.values()];
}

/** Charge seulement l'activité récente pour éviter une liste sans fin. */
async function loadJobs() {
  state.jobs = await fetchJson("/api/v1/jobs?limit=12");
  renderJobs(state.jobs);
}

/** Charge un journal court afin de garder le panneau d'activité rapide. */
async function loadServerLogs() {
  state.serverLogs = await fetchJson("/api/v1/logs?limit=40");
  renderServerLogs(state.serverLogs);
}

/** Construit les options de couches depuis le catalogue renvoyé par l'API. */
function renderLayerOptions(layers) {
  elements.layersList.replaceChildren();
  for (const layer of layers) {
    const label = document.createElement("label");
    label.className = "layer-option";
    const input = document.createElement("input");
    input.type = "checkbox";
    input.name = "layers";
    input.value = layer.id;
    input.checked = true;
    input.addEventListener("change", updateLayerSelection);
    const symbol = document.createElement("span");
    symbol.className = "layer-symbol";
    symbol.textContent = LAYER_SYMBOLS[layer.id] || layer.id.slice(0, 3).toUpperCase();
    const text = document.createElement("span");
    text.textContent = layer.label;
    label.append(input, symbol, text);
    elements.layersList.append(label);
  }
  updateLayerSelection();
}

/** Sélectionne ou désélectionne toutes les couches visibles. */
function toggleAllLayers() {
  document.querySelectorAll('input[name="layers"]').forEach((input) => {
    input.checked = elements.selectAllLayers.checked;
  });
  updateLayerSelection();
}

/** Met à jour le compteur et l'état intermédiaire de la sélection globale. */
function updateLayerSelection() {
  const inputs = [...document.querySelectorAll('input[name="layers"]')];
  const selected = inputs.filter((input) => input.checked).length;
  elements.selectedLayerCount.textContent = `${selected} couche${selected > 1 ? "s" : ""}`;
  elements.selectAllLayers.checked = selected === inputs.length && inputs.length > 0;
  elements.selectAllLayers.indeterminate = selected > 0 && selected < inputs.length;
  elements.submitDownload.disabled = selected === 0;
}

/** Applique un style distinct aux zones complètes, partielles et encore actives. */
function coverageStyle(feature) {
  const properties = feature.properties || {};
  const status = properties.status;
  if (status === "completed") {
    const isComplete =
      state.layerCatalog.length > 0 &&
      (properties.layers?.length || 0) >= state.layerCatalog.length;
    if (isComplete) {
      return { color: "#285e4d", weight: 2, fillColor: "#5f9276", fillOpacity: 0.24 };
    }
    return {
      color: "#176f83",
      weight: 2,
      dashArray: "8 4",
      fillColor: "#67a7b5",
      fillOpacity: 0.22,
    };
  }
  return {
    color: "#b16c18",
    weight: 2,
    dashArray: "6 5",
    fillColor: "#d9a759",
    fillOpacity: 0.2,
  };
}

/** Attache une fiche sûre à chaque polygone sans injecter de HTML externe. */
function bindCoveragePopup(feature, layer) {
  layer.bindPopup(() => createCoveragePopup(feature));
}

/** Produit le contenu DOM de la fiche d'une zone. */
function createCoveragePopup(feature) {
  const properties = feature.properties || {};
  const availableLayers = new Set(properties.layers || []);
  const isComplete =
    state.layerCatalog.length > 0 && availableLayers.size >= state.layerCatalog.length;
  const container = document.createElement("div");
  container.className = "coverage-popup";
  const kicker = document.createElement("p");
  kicker.className = "popup-kicker";
  kicker.textContent = STATUS_LABELS[properties.status] || properties.status || "Zone";
  const title = document.createElement("h3");
  title.textContent = properties.zone_name || `Zone de ${formatDistance(properties.size_m)}`;
  const sizeLabel = document.createElement("p");
  sizeLabel.className = "coverage-zone-size";
  sizeLabel.textContent = `${formatDistance(properties.size_m)} × ${formatDistance(properties.size_m)}`;
  const stateLabel = document.createElement("p");
  stateLabel.className = `coverage-state ${isComplete ? "is-complete" : "is-partial"}`;
  stateLabel.textContent = isComplete
    ? `Complet · ${availableLayers.size} couches`
    : `Partiel · ${availableLayers.size} / ${state.layerCatalog.length || "?"} couches`;
  const layerTitle = document.createElement("p");
  layerTitle.className = "coverage-layer-title";
  layerTitle.textContent = "Données disponibles";
  const layerList = document.createElement("div");
  layerList.className = "coverage-layer-list";
  for (const layer of state.layerCatalog) {
    const chip = document.createElement("span");
    chip.className = `coverage-layer-chip ${availableLayers.has(layer.id) ? "is-available" : "is-missing"}`;
    chip.textContent = `${availableLayers.has(layer.id) ? "✓" : "–"} ${layer.label}`;
    layerList.append(chip);
  }
  if (!state.layerCatalog.length) {
    const chip = document.createElement("span");
    chip.className = "coverage-layer-chip is-available";
    chip.textContent = `${availableLayers.size} couche${availableLayers.size > 1 ? "s" : ""}`;
    layerList.append(chip);
  }
  const note = document.createElement("p");
  note.className = "coverage-storage-note";
  note.textContent = properties.cache_hit
    ? "Les fichiers déjà présents ont été réutilisés."
    : "Les fichiers sont partagés par couche dans le stockage local.";
  const actions = document.createElement("div");
  actions.className = "coverage-actions";
  const extendButton = document.createElement("button");
  extendButton.type = "button";
  extendButton.className = "secondary-button compact-button";
  extendButton.textContent = "Étendre la zone";
  extendButton.addEventListener("click", (event) => {
    event.stopPropagation();
    extendCoverage(feature);
  });
  actions.append(extendButton);
  if (properties.status === "completed") {
    const exportButton = document.createElement("button");
    exportButton.type = "button";
    exportButton.className = "primary-button compact-button";
    exportButton.textContent = "Créer un export";
    exportButton.addEventListener("click", (event) => {
      event.stopPropagation();
      openExportForCoverage(feature);
    });
    actions.append(exportButton);
    const renameButton = document.createElement("button");
    renameButton.type = "button";
    renameButton.className = "secondary-button compact-button";
    renameButton.textContent = "Renommer";
    renameButton.addEventListener("click", (event) => {
      event.stopPropagation();
      renameCoverage(feature);
    });
    actions.append(renameButton);
    const deleteButton = document.createElement("button");
    deleteButton.type = "button";
    deleteButton.className = "danger-button compact-button";
    deleteButton.textContent = "Supprimer";
    deleteButton.addEventListener("click", (event) => {
      event.stopPropagation();
      deleteCoverage(feature);
    });
    actions.append(deleteButton);
  }
  container.append(kicker, title, sizeLabel, stateLabel, layerTitle, layerList, note, actions);
  return container;
}

/** Renomme uniquement la fiche de zone et conserve toutes les données existantes. */
async function renameCoverage(feature) {
  const id = feature.properties?.job_id;
  const current = feature.properties?.zone_name || "";
  const name = window.prompt("Nouveau nom de la zone", current)?.trim();
  if (!id || !name || name === current) {
    return;
  }
  try {
    await fetchJson(`/api/v1/zones/${encodeURIComponent(id)}`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name }),
    });
    state.map?.closePopup();
    showToast(`Zone renommée « ${name} ».`);
    await refreshLiveData();
  } catch (error) {
    showToast(normalizeError(error), true);
  }
}

/** Supprime seulement la fiche de zone après une confirmation explicite. */
async function deleteCoverage(feature) {
  const id = feature.properties?.job_id;
  if (!id) {
    showToast("Cette zone ne possède pas d’identifiant supprimable.", true);
    return;
  }
  const confirmed = window.confirm(
    "Supprimer cette zone et ses extractions propres ? Les tuiles partagées, déjà dédupliquées, resteront disponibles pour les autres zones.",
  );
  if (!confirmed) {
    return;
  }
  state.map?.closePopup();
  try {
    await fetchJson(`/api/v1/zones/${encodeURIComponent(id)}`, { method: "DELETE" });
    showToast("Zone supprimée avec ses données propres. Les tuiles partagées sont conservées.");
    await refreshLiveData();
  } catch (error) {
    showToast(normalizeError(error), true);
  }
}

/** Reprend le centre d'une zone et choisit automatiquement la taille supérieure. */
function extendCoverage(feature) {
  const properties = feature.properties || {};
  const center = properties.center;
  if (!Array.isArray(center) || center.length !== 2) {
    showToast("Le centre de cette zone n’est pas disponible.", true);
    return;
  }
  setSelection(center[0], center[1]);
  selectLargerSize(properties.size_m);
  applyDownloadLayers(properties.layers || []);
  elements.zoneName.value = properties.zone_name
    ? `${properties.zone_name} · étendue`
    : `Zone de ${formatDistance(properties.size_m)} · étendue`;
  state.map?.closePopup();
  openPanel("download");
  focusSelection();
}

/** Coche la première taille supérieure disponible, ou conserve la taille maximale. */
function selectLargerSize(currentSize) {
  const inputs = [...document.querySelectorAll('input[name="size"]')].sort(
    (left, right) => Number(left.value) - Number(right.value),
  );
  const target = inputs.find((input) => Number(input.value) > Number(currentSize)) || inputs.at(-1);
  if (!target) {
    return;
  }
  target.checked = true;
  state.selection.sizeM = Number(target.value);
}

/** Reprend les couches d'une zone existante dans le formulaire d'extension. */
function applyDownloadLayers(layers) {
  const available = new Set(layers);
  document.querySelectorAll('input[name="layers"]').forEach((input) => {
    input.checked = available.has(input.value);
  });
  updateLayerSelection();
}

/** Ouvre directement l'assistant d'export sur une zone terminée. */
function openExportForCoverage(feature) {
  state.map?.closePopup();
  openPanel("exports");
  window.ExportWizard?.selectSource(feature.properties?.job_id);
}

/** Repositionne la carte autour des emprises connues, si elles existent. */
function fitCoverage() {
  if (state.coverageLayer?.getLayers().length) {
    state.map.fitBounds(state.coverageLayer.getBounds(), { padding: [45, 45], maxZoom: 10 });
  } else {
    showToast("Aucune zone disponible à cadrer.");
  }
}

/** Met à jour le centre choisi et redessine son carré. */
function setSelection(latitude, longitude) {
  state.selection.latitude = clamp(Number(latitude), 41, 52);
  state.selection.longitude = clamp(Number(longitude), -6, 10);
  elements.latitude.value = state.selection.latitude.toFixed(6);
  elements.longitude.value = state.selection.longitude.toFixed(6);
  drawSelection();
}

/** Relit les deux champs numériques après une saisie manuelle. */
function updateSelectionFromInputs() {
  if (elements.latitude.validity.valid && elements.longitude.validity.valid) {
    setSelection(elements.latitude.value, elements.longitude.value);
  }
}

/** Dessine une emprise métrique et son centre, sans charger de donnée détaillée. */
function drawSelection() {
  if (!state.map || state.panel !== "download") {
    return;
  }
  if (state.selectionLayer) {
    state.map.removeLayer(state.selectionLayer);
  }
  if (state.selectionMarker) {
    state.map.removeLayer(state.selectionMarker);
  }
  const bounds = squareBounds(
    state.selection.latitude,
    state.selection.longitude,
    state.selection.sizeM,
  );
  state.selectionLayer = L.rectangle(bounds, {
    color: "#145f70",
    weight: 3,
    dashArray: "9 6",
    fillColor: "#4d93a3",
    fillOpacity: 0.22,
    interactive: false,
  }).addTo(state.map);
  state.selectionLayer.bindTooltip(
    `${formatDistance(state.selection.sizeM)} × ${formatDistance(state.selection.sizeM)}`,
    {
      permanent: true,
      direction: "top",
      className: "selection-size-label",
      offset: [0, -12],
    },
  );
  const icon = L.divIcon({
    className: "selection-center-icon",
    html: "<span></span>",
    iconSize: [34, 34],
    iconAnchor: [17, 17],
  });
  state.selectionMarker = L.marker(
    [state.selection.latitude, state.selection.longitude],
    { icon, interactive: false, zIndexOffset: 1000 },
  ).addTo(state.map);
}

/** Cadre le carré sélectionné pour qu'il reste entièrement visible à côté du panneau. */
function focusSelection() {
  if (!state.map || !state.selectionLayer || state.panel !== "download") {
    return;
  }
  state.map.fitBounds(state.selectionLayer.getBounds(), {
    padding: [72, 72],
    maxZoom: 18,
  });
}

/** Convertit une taille en mètres vers les degrés autour d'une latitude. */
function squareBounds(latitude, longitude, sizeM) {
  const half = sizeM / 2;
  const latitudeDelta = half / 111320;
  const longitudeDelta = half / (111320 * Math.max(Math.abs(Math.cos((latitude * Math.PI) / 180)), 0.01));
  return [
    [latitude - latitudeDelta, longitude - longitudeDelta],
    [latitude + latitudeDelta, longitude + longitudeDelta],
  ];
}

/** Envoie une demande courte à l'API puis actualise immédiatement le suivi. */
async function submitDownload(event) {
  event.preventDefault();
  const layers = [...document.querySelectorAll('input[name="layers"]:checked')].map(
    (input) => input.value,
  );
  if (!layers.length) {
    showToast("Sélectionnez au moins une couche.", true);
    return;
  }
  const name = elements.zoneName.value.trim();
  if (!name) {
    showToast("Donnez un nom à la zone pour pouvoir la retrouver.", true);
    elements.zoneName.focus();
    return;
  }
  setSubmitState(true);
  try {
    const job = await fetchJson("/api/v1/downloads", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        name,
        area: {
          center: [state.selection.latitude, state.selection.longitude],
          size_m: state.selection.sizeM,
        },
        layers,
      }),
    });
    showToast(
      job.cache_hit
        ? "Cette zone était déjà disponible : le cache a été réutilisé."
        : "Téléchargement ajouté à la file de travail.",
    );
    closePanels();
    await refreshLiveData();
  } catch (error) {
    updateConnection(false);
    showToast(normalizeError(error), true);
  } finally {
    setSubmitState(false);
  }
}

/** Désactive le bouton pendant l'envoi sans changer la mise en page. */
function setSubmitState(loading) {
  elements.submitDownload.disabled = loading;
  elements.submitDownload.querySelector(".button-label").hidden = loading;
  elements.submitDownload.querySelector(".button-loading").hidden = !loading;
}

/** Affiche trois compteurs stables dans le panneau d'activité. */
function renderActivitySummary(counters) {
  elements.activitySummary.replaceChildren();
  for (const [label, value] of [
    ["En attente", counters.queued],
    ["En cours", counters.running],
    ["Terminés", counters.completed],
  ]) {
    const item = document.createElement("div");
    item.className = "activity-stat";
    const strong = document.createElement("strong");
    strong.textContent = String(value);
    const span = document.createElement("span");
    span.textContent = label;
    item.append(strong, span);
    elements.activitySummary.append(item);
  }
}

/** Dessine la liste bornée des travaux récents. */
function renderJobs(jobs) {
  elements.jobList.replaceChildren();
  if (!jobs.length) {
    const empty = document.createElement("p");
    empty.className = "empty-list";
    empty.textContent = "Aucun traitement pour le moment. Les nouveaux téléchargements apparaîtront ici.";
    elements.jobList.append(empty);
    return;
  }
  for (const job of jobs) {
    const card = document.createElement("article");
    card.className = "job-card";
    const icon = document.createElement("div");
    icon.className = "job-icon";
    icon.textContent = job.kind?.type === "export" ? "EXP" : "DL";
    const copy = document.createElement("div");
    copy.className = "job-copy";
    const title = document.createElement("strong");
    title.textContent = job.kind?.type === "export"
      ? "Export géographique"
      : job.kind?.request?.name || "Téléchargement de zone";
    const detail = document.createElement("p");
    detail.textContent = job.message || formatDate(job.created_at);
    if (job.status === "running") {
      detail.textContent += ` · ${job.progress_percent || 0} %`;
    }
    copy.append(title, detail);
    if (["queued", "running"].includes(job.status)) {
      const progress = document.createElement("div");
      progress.className = "job-progress";
      progress.setAttribute("role", "progressbar");
      progress.setAttribute("aria-label", `Avancement de ${title.textContent}`);
      progress.setAttribute("aria-valuemin", "0");
      progress.setAttribute("aria-valuemax", "100");
      progress.setAttribute("aria-valuenow", String(job.progress_percent || 0));
      const fill = document.createElement("span");
      fill.style.width = `${Math.max(0, Math.min(100, job.progress_percent || 0))}%`;
      progress.append(fill);
      copy.append(progress);
    }
    const status = document.createElement("span");
    status.className = `job-status is-${job.status}`;
    status.textContent = STATUS_LABELS[job.status] || job.status;
    const actions = document.createElement("div");
    actions.className = "job-actions";
    actions.append(status);
    const exportArtifact = (job.artifacts || []).find((artifact) =>
      ["stl", "blender"].includes(artifact.kind),
    );
    if (job.status === "completed" && exportArtifact) {
      const download = document.createElement("a");
      download.className = "export-action-button activity-download-button";
      download.href = `/api/v1/files/${encodeURIComponent(exportArtifact.id)}/content`;
      download.download = exportArtifact.name;
      download.setAttribute("aria-label", `Télécharger ${exportArtifact.name}`);
      download.title = `Télécharger ${exportArtifact.name}`;
      download.innerHTML = `
        <svg viewBox="0 0 20 20" aria-hidden="true">
          <path d="M10 3v9M6.5 9.5 10 13l3.5-3.5M3 16.5h14" />
        </svg>
        <span>Télécharger</span>
      `;
      actions.append(download);
    }
    if (["queued", "running"].includes(job.status)) {
      const cancel = document.createElement("button");
      cancel.type = "button";
      cancel.className = "cancel-job-button";
      cancel.textContent = "Annuler";
      cancel.addEventListener("click", () => cancelJob(job.id, cancel));
      actions.append(cancel);
    }
    card.append(icon, copy, actions);
    elements.jobList.append(card);
  }
}

/** Demande l'arrêt d'un worker et rafraîchit aussitôt son état. */
async function cancelJob(jobId, button) {
  button.disabled = true;
  button.textContent = "Annulation…";
  try {
    await fetchJson(`/api/v1/jobs/${encodeURIComponent(jobId)}/cancel`, { method: "POST" });
    showToast("Annulation demandée.");
    await refreshLiveData();
  } catch (error) {
    showToast(normalizeError(error), true);
    button.disabled = false;
    button.textContent = "Annuler";
  }
}

/** Affiche les actions serveur en mettant les erreurs en évidence. */
function renderServerLogs(entries) {
  const selection = window.getSelection();
  if (
    selection
    && !selection.isCollapsed
    && selection.anchorNode
    && elements.serverLogList.contains(selection.anchorNode)
  ) {
    return;
  }
  const detailState = new Map(
    Array.from(elements.serverLogList.querySelectorAll(".server-log-row[data-log-id]")).map((row) => {
      const details = row.querySelector("details");
      const content = row.querySelector("pre");
      return [row.dataset.logId, {
        open: Boolean(details?.open),
        scrollTop: content?.scrollTop || 0,
      }];
    }),
  );
  const listScrollTop = elements.serverLogList.scrollTop;
  elements.serverLogList.replaceChildren();
  if (!entries.length) {
    const empty = document.createElement("p");
    empty.className = "empty-list";
    empty.textContent = "Le journal est vide pour le moment.";
    elements.serverLogList.append(empty);
    return;
  }
  for (const entry of entries) {
    const row = document.createElement("article");
    row.className = `server-log-row is-${entry.level}`;
    row.dataset.logId = entry.id;
    const marker = document.createElement("span");
    marker.className = "server-log-marker";
    const copy = document.createElement("div");
    const heading = document.createElement("strong");
    heading.textContent = serverActionLabel(entry.action);
    const message = document.createElement("p");
    const fullMessage = entry.message || "";
    const firstLine = fullMessage.split("\n", 1)[0];
    const hasDetails = fullMessage.includes("\n") || fullMessage.length > 180;
    message.textContent = hasDetails ? firstLine : fullMessage;
    copy.append(heading, message);
    if (hasDetails) {
      const details = document.createElement("details");
      details.className = "server-log-details";
      const summary = document.createElement("summary");
      summary.textContent = "Afficher le détail complet";
      const content = document.createElement("pre");
      content.textContent = fullMessage;
      const previousState = detailState.get(entry.id);
      details.open = Boolean(previousState?.open);
      details.append(summary, content);
      copy.append(details);
      if (previousState?.scrollTop) {
        window.requestAnimationFrame(() => {
          content.scrollTop = previousState.scrollTop;
        });
      }
    }
    const time = document.createElement("time");
    time.dateTime = entry.at;
    time.textContent = formatDate(entry.at);
    row.append(marker, copy, time);
    elements.serverLogList.append(row);
  }
  elements.serverLogList.scrollTop = listScrollTop;
}

/** Traduit les codes du journal sans masquer le message technique utile. */
function serverActionLabel(action) {
  return {
    server_started: "Serveur démarré",
    job_queued: "Travail mis en file",
    cache_hit: "Cache réutilisé",
    job_started: "Traitement démarré",
    job_progress: "Étape en cours",
    job_completed: "Traitement terminé",
    job_completed_with_warning: "Traitement terminé avec avertissement",
    job_failed: "Erreur de traitement",
    job_cancel_requested: "Annulation demandée",
    job_cancelled: "Traitement annulé",
    zone_deleted: "Zone supprimée",
    zone_pruned: "Zone redondante fusionnée",
    zone_prune_failed: "Nettoyage incomplet",
  }[action] || action;
}

/** Effectue un appel JSON et restitue le message d'erreur métier de l'API. */
async function fetchJson(url, options = {}) {
  const response = await fetch(url, options);
  if (!response.ok) {
    let message = `Erreur HTTP ${response.status}`;
    try {
      const body = await response.json();
      message = body.message || message;
    } catch {
      // Une réponse non JSON conserve le message HTTP générique.
    }
    throw new Error(message);
  }
  if (response.status === 204) {
    return null;
  }
  return response.json();
}

/** Montre l'état réel de l'API dans la barre supérieure. */
function updateConnection(online) {
  elements.connectionStatus.classList.toggle("is-online", online);
  elements.connectionStatus.classList.toggle("is-offline", !online);
  elements.connectionStatus.querySelector(".connection-label").textContent = online
    ? "API disponible"
    : "API indisponible";
}

/** Anime l'icône de rafraîchissement pendant un chargement manuel. */
function setRefreshState(loading) {
  elements.refreshData.disabled = loading;
  elements.refreshData.classList.toggle("is-spinning", loading);
}

/** Affiche une notification courte et non bloquante. */
function showToast(message, error = false) {
  const toast = document.createElement("div");
  toast.className = `toast${error ? " is-error" : ""}`;
  toast.textContent = message;
  elements.toastRegion.append(toast);
  window.setTimeout(() => toast.remove(), 5200);
}

/** Formate une taille sans précision inutile. */
function formatBytes(bytes) {
  const value = Number(bytes) || 0;
  if (value < 1024) return `${value} o`;
  const units = ["Ko", "Mo", "Go", "To"];
  let scaled = value / 1024;
  let index = 0;
  while (scaled >= 1024 && index < units.length - 1) {
    scaled /= 1024;
    index += 1;
  }
  return `${scaled >= 10 ? scaled.toFixed(0) : scaled.toFixed(1)} ${units[index]}`;
}

/** Formate une longueur carrée dans l'unité la plus parlante. */
function formatDistance(meters) {
  const value = Number(meters) || 0;
  return value >= 1000 ? `${value / 1000} km` : `${value} m`;
}

/** Formate une date de job selon la locale française. */
function formatDate(value) {
  if (!value) return "Date inconnue";
  return new Intl.DateTimeFormat("fr-FR", {
    day: "2-digit",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value));
}

/** Ramène un nombre dans l'intervalle autorisé pour la France métropolitaine. */
function clamp(value, minimum, maximum) {
  return Math.min(Math.max(value, minimum), maximum);
}

/** Transforme toute erreur inconnue en texte destiné à l'utilisateur. */
function normalizeError(error) {
  if (error instanceof Error) {
    return error.message;
  }
  return "Une erreur inattendue est survenue.";
}

document.addEventListener("DOMContentLoaded", initialize);
