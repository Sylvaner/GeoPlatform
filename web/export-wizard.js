"use strict";

/**
 * Assistant d'export isolé du contrôleur cartographique.
 * La dépendance au reste de la page passe uniquement par les fonctions fournies à initialize.
 */
window.ExportWizard = (() => {
  const LAST_EXPORT_SETTINGS_KEY = "geo-platform.last-export-settings.v1";
  const state = {
    initialized: false,
    step: 1,
    sourceId: null,
    sources: [],
    presets: [],
    history: [],
    outputNameTouched: false,
    suggestedOutputName: null,
    areaSourceId: null,
    lastLayerIds: null,
    forceExport: false,
  };
  const elements = {};
  let dependencies = null;

  /** Initialise une seule fois les événements et les valeurs de l'assistant. */
  function initialize(options) {
    if (state.initialized) {
      return;
    }
    dependencies = options;
    cacheElements();
    bindEvents();
    state.initialized = true;
    loadConfiguredDefaults().finally(() => {
      restoreLastSettings();
      refreshPreview();
    });
  }

  /** Mémorise les éléments du wizard afin de garder les autres fonctions courtes. */
  function cacheElements() {
    for (const id of [
      "export-form",
      "export-title",
      "export-progress",
      "export-show-wizard",
      "export-show-history",
      "export-wizard-view",
      "export-history-view",
      "export-history-list",
      "refresh-exports",
      "export-presets",
      "export-source-list",
      "export-area-card",
      "export-area-size",
      "export-area-size-output",
      "export-area-latitude",
      "export-area-longitude",
      "export-layer-list",
      "export-back",
      "export-next",
      "export-submit",
      "export-road-width",
      "road-width-output",
      "export-building-scale",
      "building-scale-output",
      "export-tree-scale",
      "tree-scale-output",
      "export-model-width",
      "model-width-output",
      "model-size-card",
      "blender-size-note",
      "export-scale-summary",
      "export-grid-summary",
      "export-points-summary",
      "export-terrain-grid",
      "terrain-grid-output",
      "export-base",
      "export-road-height",
      "export-road-elevation",
      "export-tree-min-height",
      "export-tree-trunk",
      "export-tree-crown",
      "export-layer-height",
      "layer-height-output",
      "export-canopy-curves",
      "export-carve-forest-roads",
      "export-keep-isolated-trees",
      "export-forest-low-color",
      "export-forest-high-color",
      "render-format-note",
      "export-include-buildings",
      "export-include-roads",
      "export-include-trees",
      "texture-mode-fieldset",
      "orthophoto-storage-fieldset",
      "export-orthophoto-resolution",
      "orthophoto-resolution-output",
      "orthophoto-resolution-help",
      "color-palette-fieldset",
      "export-ground-color",
      "export-wall-color",
      "export-roof-color",
      "export-tree-color",
      "export-road-color",
      "export-water-color",
      "export-procedural-materials",
      "export-review",
      "export-output-name",
      "export-preset-name",
    ]) {
      elements[toCamelCase(id)] = document.getElementById(id);
    }
  }

  /** Charge la palette centrale du serveur avant les préférences du dernier export. */
  async function loadConfiguredDefaults() {
    try {
      const config = await dependencies.fetchJson("/api/v1/config");
      const colors = config?.render?.colors || {};
      setValue(elements.exportGroundColor, rgbaToHex(colors.ground), elements.exportGroundColor.value);
      setValue(elements.exportWallColor, rgbaToHex(colors.walls), elements.exportWallColor.value);
      setValue(elements.exportRoofColor, rgbaToHex(colors.roofs), elements.exportRoofColor.value);
      setValue(elements.exportTreeColor, rgbaToHex(colors.trees), elements.exportTreeColor.value);
      setValue(elements.exportRoadColor, rgbaToHex(colors.roads), elements.exportRoadColor.value);
      setValue(elements.exportWaterColor, rgbaToHex(colors.water), elements.exportWaterColor.value);
      setValue(elements.exportForestLowColor, rgbaToHex(colors.forest_low), elements.exportForestLowColor.value);
      setValue(elements.exportForestHighColor, rgbaToHex(colors.forest_high), elements.exportForestHighColor.value);
    } catch {
      // Les valeurs HTML gardent l'interface utilisable si la configuration est indisponible.
    }
  }

  /** Convertit un identifiant HTML avec tirets en propriété JavaScript. */
  function toCamelCase(value) {
    return value.replace(/-([a-z])/g, (_, letter) => letter.toUpperCase());
  }

  /** Recharge automatiquement les choix du dernier export réussi, sans reprendre sa zone. */
  function restoreLastSettings() {
    try {
      const saved = JSON.parse(window.localStorage.getItem(LAST_EXPORT_SETTINGS_KEY) || "null");
      if (!saved?.advanced) return;
      const advanced = saved.advanced;
      state.lastLayerIds = Array.isArray(saved.layers) ? saved.layers : null;
      setRadioValue("export-format", saved.format || "stl");
      setValue(elements.exportTerrainGrid, advanced.terrain_grid_step_m, 50);
      elements.exportIncludeBuildings.checked = advanced.include_buildings !== false;
      elements.exportIncludeRoads.checked = advanced.include_roads !== false;
      elements.exportIncludeTrees.checked = advanced.include_trees !== false;
      setValue(elements.exportRoadWidth, advanced.road_min_width_mm, 1);
      setValue(elements.exportBuildingScale, advanced.vertical_exaggeration, 1);
      setValue(elements.exportTreeScale, advanced.tree_height_scale, 1);
      setValue(elements.exportModelWidth, advanced.width_mm, 150);
      setValue(elements.exportBase, advanced.base_mm, 3);
      setValue(elements.exportRoadHeight, advanced.road_height_mm, 0.6);
      setValue(elements.exportRoadElevation, advanced.road_elevation_m, 0);
      setValue(elements.exportTreeMinHeight, advanced.tree_min_height_mm, 5);
      setValue(elements.exportTreeTrunk, advanced.tree_trunk_diameter_mm, 1.2);
      setValue(elements.exportTreeCrown, advanced.tree_crown_diameter_mm, 3);
      setValue(elements.exportLayerHeight, advanced.layer_height_m, 1);
      setRadioValue("export-tree-shape", advanced.tree_shape || "deciduous");
      setRadioValue("export-forest-quality", advanced.forest_quality || "standard");
      setRadioValue("export-forest-material", advanced.forest_material_mode || "procedural");
      elements.exportCanopyCurves.checked = advanced.canopy_smoothing !== 0;
      elements.exportCarveForestRoads.checked = advanced.carve_forest_roads === true;
      elements.exportKeepIsolatedTrees.checked = advanced.keep_isolated_trees !== false;
      setRadioValue("export-texture-mode", advanced.texture_mode || "ground_and_roofs");
      setRadioValue("export-orthophoto-storage", advanced.orthophoto_storage || "atlas");
      setValue(elements.exportOrthophotoResolution, advanced.orthophoto_resolution_px, 2048);
      elements.exportProceduralMaterials.checked = advanced.procedural_materials !== false;
      const colors = advanced.colors || {};
      setValue(elements.exportGroundColor, rgbaToHex(colors.ground), elements.exportGroundColor.value);
      setValue(elements.exportWallColor, rgbaToHex(colors.walls), elements.exportWallColor.value);
      setValue(elements.exportRoofColor, rgbaToHex(colors.roofs), elements.exportRoofColor.value);
      setValue(elements.exportTreeColor, rgbaToHex(colors.trees), elements.exportTreeColor.value);
      setValue(elements.exportRoadColor, rgbaToHex(colors.roads), elements.exportRoadColor.value);
      setValue(elements.exportWaterColor, rgbaToHex(colors.water), elements.exportWaterColor.value);
      setValue(elements.exportForestLowColor, rgbaToHex(colors.forest_low), elements.exportForestLowColor.value);
      setValue(elements.exportForestHighColor, rgbaToHex(colors.forest_high), elements.exportForestHighColor.value);
    } catch {
      window.localStorage.removeItem(LAST_EXPORT_SETTINGS_KEY);
    }
  }

  /** Mémorise seulement les choix reproductibles après acceptation du job par le serveur. */
  function saveLastSettings(format, layers, advanced) {
    try {
      window.localStorage.setItem(
        LAST_EXPORT_SETTINGS_KEY,
        JSON.stringify({ format, layers, advanced }),
      );
      state.lastLayerIds = [...layers];
    } catch {
      // Un stockage navigateur désactivé ne doit jamais bloquer un export.
    }
  }

  /** Relie les boutons, formats et curseurs à l'état du wizard. */
  function bindEvents() {
    elements.exportBack.addEventListener("click", () => setStep(state.step - 1));
    elements.exportNext.addEventListener("click", () => setStep(state.step + 1));
    elements.exportForm.addEventListener("submit", submitExport);
    elements.exportShowWizard.addEventListener("click", startWizard);
    elements.exportShowHistory.addEventListener("click", () => showView("history"));
    elements.refreshExports.addEventListener("click", loadHistory);
    elements.exportOutputName.addEventListener("input", () => {
      state.outputNameTouched = true;
    });
    elements.exportAreaSize.addEventListener("input", exportAreaChanged);
    elements.exportAreaLatitude.addEventListener("change", exportAreaChanged);
    elements.exportAreaLongitude.addEventListener("change", exportAreaChanged);
    elements.exportLayerList.addEventListener("change", () => {
      updateObjectToggleAvailability();
      updateFormatSpecificFields();
      refreshPreview();
    });
    for (const input of [
      elements.exportIncludeBuildings,
      elements.exportIncludeRoads,
      elements.exportIncludeTrees,
    ]) {
      input.addEventListener("change", () => {
        updateFormatSpecificFields();
        refreshPreview();
      });
    }
    document.querySelectorAll('input[name="export-format"]').forEach((input) => {
      input.addEventListener("change", () => {
        updateFormatSpecificFields();
        updateOutputName();
        updateDimensionSummary();
        refreshPreview();
      });
    });
    for (const input of [
      elements.exportRoadWidth,
      elements.exportRoadElevation,
      elements.exportBuildingScale,
      elements.exportTreeScale,
    ]) {
      input.addEventListener("input", refreshPreview);
    }
    elements.exportModelWidth.addEventListener("input", () => {
      updateDimensionSummary();
      refreshPreview();
    });
    elements.exportLayerHeight.addEventListener("input", () => {
      updateDimensionSummary();
      updateOutputName(true);
      refreshPreview();
    });
    elements.exportTerrainGrid.addEventListener("input", () => {
      updateDimensionSummary();
      refreshPreview();
    });
    document.querySelectorAll('input[name="export-tree-shape"]').forEach((input) => {
      input.addEventListener("change", () => {
        updateFormatSpecificFields();
        refreshPreview();
      });
    });
    document.querySelectorAll('input[name="export-forest-quality"]').forEach((input) => {
      input.addEventListener("change", refreshPreview);
    });
    elements.exportCanopyCurves.addEventListener("change", refreshPreview);
    elements.exportCarveForestRoads.addEventListener("change", refreshPreview);
    elements.exportKeepIsolatedTrees.addEventListener("change", refreshPreview);
    document.querySelectorAll('input[name="export-forest-material"]').forEach((input) => {
      input.addEventListener("change", () => {
        updateFormatSpecificFields();
        refreshPreview();
      });
    });
    document.querySelectorAll('input[name="export-texture-mode"]').forEach((input) => {
      input.addEventListener("change", () => {
        updateFormatSpecificFields();
        refreshPreview();
      });
    });
    document.querySelectorAll('input[name="export-orthophoto-storage"]').forEach((input) => {
      input.addEventListener("change", () => {
        updateFormatSpecificFields();
        refreshOrthophotoResolution();
      });
    });
    elements.exportOrthophotoResolution.addEventListener("input", refreshOrthophotoResolution);
    elements.exportProceduralMaterials.addEventListener("change", refreshPreview);
    for (const input of [
      elements.exportGroundColor,
      elements.exportWallColor,
      elements.exportRoofColor,
      elements.exportTreeColor,
      elements.exportForestLowColor,
      elements.exportForestHighColor,
      elements.exportRoadColor,
      elements.exportWaterColor,
    ]) {
      input.addEventListener("input", refreshPreview);
    }
    document.querySelectorAll("[data-model-width]").forEach((button) => {
      button.addEventListener("click", () => {
        elements.exportModelWidth.value = button.dataset.modelWidth;
        updateDimensionSummary();
        refreshPreview();
      });
    });
  }

  /** Ouvre le panneau sur les fichiers récents, qui constituent son usage principal. */
  async function open() {
    if (!state.initialized) {
      return;
    }
    showView("history");
    state.sourceId = null;
    state.outputNameTouched = false;
    state.suggestedOutputName = null;
    refreshSources();
    await Promise.all([loadPresets(), loadHistory()]);
    setStep(1);
  }

  /** Démarre une nouvelle configuration sans conserver un ancien nom de fichier. */
  function startWizard() {
    state.forceExport = false;
    state.outputNameTouched = false;
    state.suggestedOutputName = null;
    refreshSources();
    setStep(1);
    showView("wizard");
  }

  /** Réinitialise seulement l'étape courante lorsque le panneau est fermé. */
  function close() {
    state.step = 1;
  }

  /** Bascule entre la création guidée et la consultation des productions. */
  function showView(view) {
    const showWizard = view === "wizard";
    elements.exportWizardView.hidden = !showWizard;
    elements.exportHistoryView.hidden = showWizard;
    elements.exportShowHistory.hidden = !showWizard;
    elements.exportTitle.textContent = showWizard ? "Nouvel export" : "Exports";
  }

  /** Reconstruit les zones exportables depuis les emprises connues de la carte. */
  function refreshSources() {
    if (!state.initialized) {
      return;
    }
    const features = dependencies.getSources().filter(
      (feature) => feature.properties?.status === "completed",
    );
    state.sources = features.map((feature) => ({
      id: feature.properties.job_id,
      label: feature.properties.zone_name || `Zone de ${formatDistance(feature.properties.size_m)}`,
      area: {
        center: feature.properties.center,
        size_m: feature.properties.size_m,
      },
      layers: feature.properties.layers || [],
      ready: true,
    }));
    const current = dependencies.getCurrentSelection();
    state.sources.push({
      id: "current-selection",
      label: "Sélection actuelle",
      area: current.area,
      layers: dependencies.getLayers().map((layer) => layer.id),
      ready: false,
    });
    if (!state.sources.some((source) => source.id === state.sourceId)) {
      state.sourceId = state.sources.find((source) => source.ready)?.id || "current-selection";
    }
    renderSources();
    configureExportArea();
    renderExportLayers();
    updateDimensionSummary();
    updateOutputName(true);
  }

  /** Présélectionne une zone lorsqu'un export est demandé depuis sa fiche cartographique. */
  function selectSource(jobId) {
    state.sourceId = jobId;
    showView("wizard");
    renderSources();
    configureExportArea(true);
    renderExportLayers();
    updateOutputName(true);
    setStep(1);
  }

  /** Charge les profils enregistrés et conserve une liste courte à parcourir. */
  async function loadPresets() {
    try {
      state.presets = await dependencies.fetchJson("/api/v1/export-presets");
      renderPresets();
    } catch (error) {
      state.presets = [];
      renderPresets();
      dependencies.showToast(normalizeError(error), true);
    }
  }

  /** Charge un historique borné pour éviter de remplir le panneau avec tous les anciens jobs. */
  async function loadHistory() {
    try {
      state.history = await dependencies.fetchJson("/api/v1/jobs?kind=export&limit=12");
      renderHistory();
    } catch (error) {
      state.history = [];
      renderHistory();
      dependencies.showToast(normalizeError(error), true);
    }
  }

  /** Présente l'état et les fichiers utiles de chaque export sans exposer les chemins internes. */
  function renderHistory() {
    elements.exportHistoryList.replaceChildren();
    if (!state.history.length) {
      const empty = document.createElement("div");
      empty.className = "export-history-empty";
      empty.textContent = "Aucun export lancé pour le moment.";
      elements.exportHistoryList.append(empty);
      return;
    }
    for (const job of state.history) {
      const request = job.kind?.request || {};
      const card = document.createElement("article");
      card.className = `export-history-card is-${job.status}`;
      const header = document.createElement("div");
      header.className = "export-history-card-header";
      const copy = document.createElement("div");
      const title = document.createElement("strong");
      title.textContent = request.output_name || `Export ${formatLabel(request.format)}`;
      const meta = document.createElement("small");
      meta.textContent = `${formatLabel(request.format)} · ${request.layers?.length || 0} couche${request.layers?.length > 1 ? "s" : ""} · ${formatDate(job.created_at)}`;
      copy.append(title, meta);
      const status = document.createElement("span");
      status.className = `export-history-status is-${job.status}`;
      status.textContent = statusLabel(job.status);
      header.append(copy);
      if (job.status !== "completed") {
        header.append(status);
      }
      const message = document.createElement("p");
      message.textContent = job.message;
      const actions = document.createElement("div");
      actions.className = "export-history-actions";
      if (["queued", "running"].includes(job.status)) {
        actions.append(cancelButton(job.id));
      } else {
        actions.append(regenerateButton(job));
      }
      for (const artifact of job.artifacts || []) {
        if (!["stl", "blender"].includes(artifact.kind)) {
          continue;
        }
        if (artifact.kind === "stl") {
          const preview = document.createElement("button");
          preview.type = "button";
          preview.className = "export-action-button is-preview";
          preview.innerHTML = `${actionIcon("view")}<span>Visualiser</span>`;
          preview.setAttribute("aria-label", `Visualiser ${artifact.name}`);
          preview.title = `Visualiser ${artifact.name}`;
          preview.addEventListener("click", async () => {
            try {
              if (!window.StlViewer) {
                throw new Error("Le visualiseur STL n’a pas pu être chargé.");
              }
              await window.StlViewer.open({
                url: `/api/v1/files/${encodeURIComponent(artifact.id)}/content`,
                name: artifact.name,
              });
            } catch (error) {
              dependencies.showToast(normalizeError(error), true);
            }
          });
          actions.append(preview);
        }
        const link = document.createElement("a");
        link.className = "export-action-button";
        link.href = `/api/v1/files/${encodeURIComponent(artifact.id)}/content`;
        link.setAttribute("aria-label", `Télécharger ${artifact.name}`);
        link.title = `Télécharger ${artifact.name}`;
        link.innerHTML = `${actionIcon("download")}<span>Télécharger</span>`;
        link.download = artifact.name;
        actions.append(link);
      }
      if (!["queued", "running"].includes(job.status)) {
        actions.append(deleteExportButton(job));
      }
      card.append(header);
      if (job.status !== "completed" && job.message) {
        card.append(message);
      }
      if (actions.childElementCount) {
        card.append(actions);
      }
      elements.exportHistoryList.append(card);
    }
  }

  /** Retourne les pictogrammes compacts utilisés dans les actions d'un export. */
  function actionIcon(name) {
    const paths = {
      download: '<path d="M10 3v9M6.5 9.5 10 13l3.5-3.5M3 16.5h14" />',
      view: '<path d="M2.5 10s2.7-5 7.5-5 7.5 5 7.5 5-2.7 5-7.5 5-7.5-5-7.5-5Z"/><circle cx="10" cy="10" r="2"/>',
      regenerate: '<path d="M15.5 7A6 6 0 1 0 16 12M15.5 3v4h-4" />',
      delete: '<path d="M4 6h12M8 3h4l1 3H7l1-3ZM6 6l.7 11h6.6L14 6M9 9v5M11 9v5" />',
    };
    return `<svg viewBox="0 0 20 20" aria-hidden="true">${paths[name] || ""}</svg>`;
  }

  /** Recharge la configuration d'un export et force sa prochaine génération. */
  function regenerateButton(job) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "export-action-button is-secondary";
    button.innerHTML = `${actionIcon("regenerate")}<span>Régénérer</span>`;
    button.addEventListener("click", () => preloadExport(job));
    return button;
  }

  /** Supprime le fichier et l'entrée d'historique après confirmation explicite. */
  function deleteExportButton(job) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "export-action-button is-danger";
    button.innerHTML = actionIcon("delete");
    button.setAttribute("aria-label", "Supprimer cet export");
    button.title = "Supprimer cet export";
    button.addEventListener("click", async () => {
      if (!window.confirm("Supprimer cet export et son fichier ?")) {
        return;
      }
      button.disabled = true;
      try {
        await dependencies.fetchJson(`/api/v1/jobs/${encodeURIComponent(job.id)}`, {
          method: "DELETE",
        });
        dependencies.showToast("Export supprimé.");
        await Promise.all([loadHistory(), dependencies.refreshData()]);
      } catch (error) {
        dependencies.showToast(normalizeError(error), true);
        button.disabled = false;
      }
    });
    return button;
  }

  /** Formate une date serveur de manière compacte dans le fuseau du navigateur. */
  function formatDate(value) {
    const date = new Date(value);
    return Number.isNaN(date.getTime())
      ? "date inconnue"
      : date.toLocaleString("fr-FR", { dateStyle: "short", timeStyle: "short" });
  }

  /** Traduit le cycle de vie d'un job en libellé utilisateur. */
  function statusLabel(status) {
    return {
      queued: "En attente",
      running: "En cours",
      completed: "Terminé",
      failed: "Échec",
      cancelled: "Annulé",
    }[status] || status;
  }

  /** Crée une action d'annulation commune aux travaux en attente et en cours. */
  function cancelButton(jobId) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "cancel-job-button";
    button.textContent = "Annuler";
    button.addEventListener("click", async () => {
      button.disabled = true;
      button.textContent = "Annulation…";
      try {
        await dependencies.fetchJson(`/api/v1/jobs/${encodeURIComponent(jobId)}/cancel`, {
          method: "POST",
        });
        dependencies.showToast("Annulation demandée.");
        await Promise.all([loadHistory(), dependencies.refreshData()]);
      } catch (error) {
        dependencies.showToast(normalizeError(error), true);
        button.disabled = false;
        button.textContent = "Annuler";
      }
    });
    return button;
  }

  /** Affiche les profils sous forme de boutons immédiatement réutilisables. */
  function renderPresets() {
    elements.exportPresets.replaceChildren();
    if (!state.presets.length) {
      const empty = document.createElement("span");
      empty.className = "preset-empty";
      empty.textContent = "Aucun profil enregistré pour le moment.";
      elements.exportPresets.append(empty);
      return;
    }
    for (const preset of state.presets) {
      const button = document.createElement("button");
      button.className = "preset-pill";
      button.type = "button";
      button.textContent = preset.name;
      button.addEventListener("click", () => applyPreset(preset));
      elements.exportPresets.append(button);
    }
  }

  /** Affiche les zones terminées et une sélection courante utilisable pour prévisualiser. */
  function renderSources() {
    elements.exportSourceList.replaceChildren();
    const readySources = state.sources.filter((source) => source.ready);
    if (!readySources.length) {
      const warning = document.createElement("div");
      warning.className = "source-warning";
      warning.textContent =
        "Aucune zone terminée n’est encore exportable. Vous pouvez préparer le rendu avec la sélection actuelle, mais le lancement restera bloqué tant que ses données ne sont pas téléchargées.";
      elements.exportSourceList.append(warning);
    }
    for (const source of state.sources) {
      const label = document.createElement("label");
      label.className = "export-source-card";
      const radio = document.createElement("input");
      radio.type = "radio";
      radio.name = "export-source";
      radio.value = source.id;
      radio.checked = source.id === state.sourceId;
      radio.addEventListener("change", () => {
        state.sourceId = source.id;
        configureExportArea(true);
        renderExportLayers();
        updateDimensionSummary();
        updateOutputName(true);
        refreshPreview();
      });
      const copy = document.createElement("span");
      copy.className = "source-copy";
      const title = document.createElement("strong");
      title.textContent = source.label;
      const coordinates = document.createElement("small");
      coordinates.textContent = `${source.area.center[0].toFixed(5)}, ${source.area.center[1].toFixed(5)}`;
      copy.append(title, coordinates);
      const layers = document.createElement("span");
      layers.className = "source-layers";
      layers.textContent = source.ready
        ? `${source.layers.length}/${dependencies.getLayers().length} couches`
        : "Aperçu seulement";
      label.append(radio, copy, layers);
      elements.exportSourceList.append(label);
    }
  }

  /** Initialise la sous-zone d'export dans la zone téléchargée choisie. */
  function configureExportArea(force = false) {
    const source = selectedSource();
    const usable = Boolean(source?.ready && source.area?.center && source.area?.size_m);
    elements.exportAreaCard.hidden = !usable;
    if (!usable) {
      return;
    }
    const changed = state.areaSourceId !== source.id;
    if (force || changed || !elements.exportAreaSize.value) {
      const maximum = Math.max(100, Math.round(Number(source.area.size_m)));
      elements.exportAreaSize.min = String(Math.min(100, maximum));
      elements.exportAreaSize.max = String(maximum);
      elements.exportAreaSize.step = maximum <= 1000 ? "50" : "100";
      elements.exportAreaSize.value = String(maximum);
      elements.exportAreaLatitude.value = Number(source.area.center[0]).toFixed(6);
      elements.exportAreaLongitude.value = Number(source.area.center[1]).toFixed(6);
      state.areaSourceId = source.id;
    }
    normalizeExportArea();
  }

  /** Répercute immédiatement une modification d'emprise dans le nom et les estimations. */
  function exportAreaChanged() {
    normalizeExportArea();
    updateDimensionSummary();
    updateOutputName(true);
  }

  /** Maintient le carré demandé entièrement dans le carré déjà téléchargé. */
  function normalizeExportArea() {
    const source = selectedSource();
    if (!source?.ready) {
      return null;
    }
    const sourceSize = Number(source.area.size_m);
    const minimum = Math.min(100, sourceSize);
    const size = Math.min(sourceSize, Math.max(minimum, Number(elements.exportAreaSize.value) || sourceSize));
    const sourceLatitude = Number(source.area.center[0]);
    const sourceLongitude = Number(source.area.center[1]);
    const latitudeScale = 111_320;
    const longitudeScale = latitudeScale * Math.max(0.2, Math.cos((sourceLatitude * Math.PI) / 180));
    const travel = Math.max(0, (sourceSize - size) / 2);
    const latitude = Math.min(
      sourceLatitude + travel / latitudeScale,
      Math.max(sourceLatitude - travel / latitudeScale, Number(elements.exportAreaLatitude.value) || sourceLatitude),
    );
    const longitude = Math.min(
      sourceLongitude + travel / longitudeScale,
      Math.max(sourceLongitude - travel / longitudeScale, Number(elements.exportAreaLongitude.value) || sourceLongitude),
    );
    elements.exportAreaSize.value = String(Math.round(size));
    elements.exportAreaLatitude.value = latitude.toFixed(6);
    elements.exportAreaLongitude.value = longitude.toFixed(6);
    elements.exportAreaSizeOutput.textContent = `${formatDistance(Math.round(size))} × ${formatDistance(Math.round(size))}`;
    return { ...source.area, center: [latitude, longitude], size_m: Math.round(size) };
  }

  /** Construit la sélection des couches en désactivant celles absentes de la zone. */
  function renderExportLayers(selectedIds = null) {
    const source = selectedSource();
    if (!source) {
      return;
    }
    const available = new Set(source.layers);
    const requested = new Set(selectedIds || state.lastLayerIds || source.layers);
    elements.exportLayerList.replaceChildren();
    for (const layer of dependencies.getLayers()) {
      const label = document.createElement("label");
      label.className = "export-layer-choice";
      const input = document.createElement("input");
      input.type = "checkbox";
      input.name = "export-layers";
      input.value = layer.id;
      input.checked = available.has(layer.id) && requested.has(layer.id);
      input.disabled = !available.has(layer.id);
      if (input.disabled) {
        label.classList.add("is-unavailable");
      }
      const text = document.createElement("span");
      text.textContent = layer.label;
      label.append(input, text);
      elements.exportLayerList.append(label);
    }
    updateObjectToggleAvailability();
  }

  /** Désactive un objet lorsque sa couche source n'est pas sélectionnée ou disponible. */
  function updateObjectToggleAvailability() {
    for (const [toggle, layerId] of objectLayerPairs()) {
      const layer = document.querySelector(`input[name="export-layers"][value="${layerId}"]`);
      toggle.disabled = !layer?.checked;
      const label = toggle.closest("label");
      if (label) {
        label.hidden = toggle.disabled;
      }
    }
  }

  /** Centralise les correspondances simples entre objet 3D et couche téléchargée. */
  function objectLayerPairs() {
    return [
      [elements.exportIncludeBuildings, "buildings"],
      [elements.exportIncludeRoads, "transport"],
      [elements.exportIncludeTrees, "lidar"],
    ];
  }

  /** Retrouve la plus petite zone téléchargée qui contient entièrement l'ancien export. */
  function sourceContainingArea(area) {
    if (!area?.center || !area?.size_m) {
      return null;
    }
    return state.sources
      .filter((source) => source.ready && areaFitsInside(source.area, area))
      .sort((first, second) => first.area.size_m - second.area.size_m)[0] || null;
  }

  /** Compare deux carrés géographiques avec une approximation métrique locale suffisante. */
  function areaFitsInside(container, candidate) {
    const latitudeScale = 111_320;
    const longitudeScale = latitudeScale * Math.max(
      0.2,
      Math.cos((Number(container.center[0]) * Math.PI) / 180),
    );
    const latitudeDistance = Math.abs(candidate.center[0] - container.center[0]) * latitudeScale;
    const longitudeDistance = Math.abs(candidate.center[1] - container.center[1]) * longitudeScale;
    const available = (Number(container.size_m) - Number(candidate.size_m)) / 2;
    return available >= -1 && latitudeDistance <= available + 1 && longitudeDistance <= available + 1;
  }

  /** Ouvre le wizard avec la zone et tous les réglages d'un export précédent. */
  function preloadExport(job) {
    const request = job.kind?.request;
    const source = sourceContainingArea(request?.area);
    if (!request || !source) {
      dependencies.showToast(
        "La zone source de cet export n’est plus disponible.",
        true,
      );
      return;
    }
    state.forceExport = true;
    state.outputNameTouched = false;
    state.suggestedOutputName = null;
    state.sourceId = source.id;
    state.areaSourceId = null;
    state.lastLayerIds = request.layers?.length ? [...request.layers] : [...source.layers];
    showView("wizard");
    renderSources();
    configureExportArea(true);
    elements.exportAreaSize.value = String(request.area.size_m);
    elements.exportAreaLatitude.value = Number(request.area.center[0]).toFixed(6);
    elements.exportAreaLongitude.value = Number(request.area.center[1]).toFixed(6);
    normalizeExportArea();
    applyPreset(
      {
        name: "export précédent",
        format: request.format,
        layers: state.lastLayerIds,
        quality: request.quality,
        advanced: request.advanced || {},
      },
      false,
    );
    updateOutputName(true);
    setStep(1);
    dependencies.showToast("Configuration de l’export rechargée.");
  }

  /** Applique un profil sans modifier la zone géographique choisie. */
  function applyPreset(preset, notify = true) {
    setRadioValue("export-format", preset.format);
    const advanced = preset.advanced || {};
    setValue(
      elements.exportTerrainGrid,
      advanced.terrain_grid_step_m,
      { draft: 100, standard: 50, high: 5 }[preset.quality] || 50,
    );
    elements.exportIncludeBuildings.checked = advanced.include_buildings !== false;
    elements.exportIncludeRoads.checked = advanced.include_roads !== false;
    elements.exportIncludeTrees.checked = advanced.include_trees !== false;
    setValue(elements.exportRoadWidth, advanced.road_min_width_mm, 1);
    setValue(elements.exportBuildingScale, advanced.vertical_exaggeration, 1);
    setValue(elements.exportTreeScale, advanced.tree_height_scale, 1);
    setValue(elements.exportModelWidth, advanced.width_mm, 150);
    setValue(elements.exportBase, advanced.base_mm, 3);
    setValue(elements.exportRoadHeight, advanced.road_height_mm, 0.6);
    setValue(elements.exportRoadElevation, advanced.road_elevation_m, 0);
    setValue(elements.exportTreeMinHeight, advanced.tree_min_height_mm, 5);
    setValue(elements.exportTreeTrunk, advanced.tree_trunk_diameter_mm, 1.2);
    setValue(elements.exportTreeCrown, advanced.tree_crown_diameter_mm, 3);
    setValue(elements.exportLayerHeight, advanced.layer_height_m, 1);
    setRadioValue("export-tree-shape", advanced.tree_shape || "deciduous");
    setRadioValue("export-forest-quality", advanced.forest_quality || "standard");
    setRadioValue("export-forest-material", advanced.forest_material_mode || "procedural");
    elements.exportCanopyCurves.checked = advanced.canopy_smoothing !== 0;
    elements.exportCarveForestRoads.checked = advanced.carve_forest_roads === true;
    elements.exportKeepIsolatedTrees.checked = advanced.keep_isolated_trees !== false;
    setRadioValue("export-texture-mode", advanced.texture_mode || "ground_and_roofs");
    setRadioValue("export-orthophoto-storage", advanced.orthophoto_storage || "atlas");
    setValue(elements.exportOrthophotoResolution, advanced.orthophoto_resolution_px, 2048);
    elements.exportProceduralMaterials.checked = advanced.procedural_materials !== false;
    const colors = advanced.colors || {};
    setValue(elements.exportGroundColor, rgbaToHex(colors.ground), elements.exportGroundColor.value);
    setValue(elements.exportWallColor, rgbaToHex(colors.walls), elements.exportWallColor.value);
    setValue(elements.exportRoofColor, rgbaToHex(colors.roofs), elements.exportRoofColor.value);
    setValue(elements.exportTreeColor, rgbaToHex(colors.trees), elements.exportTreeColor.value);
    setValue(elements.exportRoadColor, rgbaToHex(colors.roads), elements.exportRoadColor.value);
    setValue(elements.exportWaterColor, rgbaToHex(colors.water), elements.exportWaterColor.value);
    setValue(elements.exportForestLowColor, rgbaToHex(colors.forest_low), elements.exportForestLowColor.value);
    setValue(elements.exportForestHighColor, rgbaToHex(colors.forest_high), elements.exportForestHighColor.value);
    renderExportLayers(preset.layers);
    updateFormatSpecificFields();
    updateOutputName();
    updateDimensionSummary();
    refreshPreview();
    if (notify) {
      dependencies.showToast(`Profil « ${preset.name} » appliqué.`);
    }
  }

  /** Affecte une valeur numérique ou son repli lisible. */
  function setValue(input, value, fallback) {
    input.value = String(value ?? fallback);
  }

  /** Sélectionne un bouton radio identifié par son nom et sa valeur. */
  function setRadioValue(name, value) {
    const input = document.querySelector(`input[name="${name}"][value="${value}"]`);
    if (input) {
      input.checked = true;
    }
  }

  /** Change d'étape et synchronise progression, boutons et résumé final. */
  function setStep(nextStep) {
    state.step = Math.max(1, Math.min(5, nextStep));
    document.querySelectorAll("[data-export-step]").forEach((section) => {
      const active = Number(section.dataset.exportStep) === state.step;
      section.hidden = !active;
      section.classList.toggle("is-active", active);
    });
    [...elements.exportProgress.children].forEach((item, index) => {
      item.classList.toggle("is-active", index + 1 === state.step);
      item.classList.toggle("is-complete", index + 1 < state.step);
    });
    elements.exportBack.hidden = state.step === 1;
    elements.exportNext.hidden = state.step === 5;
    elements.exportSubmit.hidden = state.step !== 5;
    if (state.step === 2) {
      renderExportLayers(selectedLayerIds());
    }
    if (state.step === 3) {
      updateDimensionSummary();
    }
    if (state.step === 4) {
      updateFormatSpecificFields();
      refreshPreview();
    }
    if (state.step === 5) {
      renderReview();
      updateSubmitAvailability();
    }
  }

  /** Met à jour les libellés puis transmet les réglages au petit moteur 3D. */
  function refreshPreview() {
    const roadWidth = Number(elements.exportRoadWidth.value);
    const buildingScale = Number(elements.exportBuildingScale.value);
    const treeScale = Number(elements.exportTreeScale.value);
    elements.roadWidthOutput.textContent = `${formatNumber(roadWidth)} mm`;
    elements.buildingScaleOutput.textContent = `× ${formatNumber(buildingScale)}`;
    elements.treeScaleOutput.textContent = `× ${formatNumber(treeScale)}`;
    refreshOrthophotoResolution();
    const config = {
      roadWidth,
      roadElevationM: Number(elements.exportRoadElevation.value),
      buildingScale,
      treeScale,
      treeShape: selectedTreeShape(),
      forestQuality: selectedForestQuality(),
      canopySmoothing: selectedCanopySmoothing(),
      forestMaterial: selectedForestMaterial(),
      forestColorLow: elements.exportForestLowColor.value,
      forestColorHigh: elements.exportForestHighColor.value,
      carveForestRoads: elements.exportCarveForestRoads.checked,
      keepIsolatedTrees: elements.exportKeepIsolatedTrees.checked,
      quality: selectedQuality(),
      format: selectedFormat(),
      layerHeightM: Number(elements.exportLayerHeight.value),
      layers: previewLayerIds(),
      widthMm: Number(elements.exportModelWidth.value),
      colors: currentColors(),
      proceduralMaterials: elements.exportProceduralMaterials.checked,
    };
    window.ExportPreview3D?.update(config);
    window.dispatchEvent(new CustomEvent("geo-export-preview", { detail: config }));
  }

  /** Affiche le compromis mémoire/résolution de l'atlas en fonction de l'emprise choisie. */
  function refreshOrthophotoResolution() {
    const pixels = Number(elements.exportOrthophotoResolution.value) || 2048;
    const side = Number(selectedExportArea()?.size_m) || 0;
    elements.orthophotoResolutionOutput.textContent = `${pixels.toLocaleString("fr-FR")} px`;
    elements.orthophotoResolutionHelp.textContent = side > 0
      ? `Une seule image · environ ${formatNumber(side / pixels)} m par pixel sur cette zone.`
      : `Une seule image de ${pixels.toLocaleString("fr-FR")} × ${pixels.toLocaleString("fr-FR")} px.`;
  }

  /** Calcule l'échelle et la densité du maillage sans exposer un réglage de points manuel. */
  function updateDimensionSummary() {
    const source = selectedSource();
    const width = Number(elements.exportModelWidth.value) || 150;
    const preferredGridStep = Number(elements.exportTerrainGrid.value) || 50;
    const side = Number(selectedExportArea()?.size_m || source?.area?.size_m) || 3000;
    const maximumAxisPoints = Math.floor(Math.sqrt(750000));
    const requiredGridStep = Math.ceil((side / (maximumAxisPoints - 1)) * 10) / 10;
    const gridStep = Math.max(preferredGridStep, requiredGridStep);
    const gridSize = Math.ceil(side / gridStep) + 1;
    const points = gridSize * gridSize;
    const blender = selectedFormat() === "blender";
    elements.modelWidthOutput.textContent = `${width} mm`;
    elements.terrainGridOutput.textContent = `${formatNumber(preferredGridStep)} m`;
    elements.layerHeightOutput.textContent = `${formatNumber(Number(elements.exportLayerHeight.value))} m`;
    elements.modelSizeCard.hidden = blender;
    elements.blenderSizeNote.hidden = !blender;
    elements.exportScaleSummary.textContent = blender
      ? "Dimensions réelles"
      : `1 : ${Math.round((side * 1000) / width).toLocaleString("fr-FR")}`;
    elements.exportGridSummary.textContent = `${formatNumber(gridStep)} m · ${gridSize.toLocaleString("fr-FR")} × ${gridSize.toLocaleString("fr-FR")}`;
    elements.exportPointsSummary.textContent = `≈ ${points.toLocaleString("fr-FR")}`;
    document.querySelectorAll("[data-model-width]").forEach((button) => {
      button.classList.toggle("is-active", Number(button.dataset.modelWidth) === width);
    });
  }

  /** Adapte les contrôles au format et aux couches réellement incluses. */
  function updateFormatSpecificFields() {
    const format = selectedFormat();
    const layered = format === "stl_layers";
    const printable = format !== "blender";
    const layers = new Set(selectedLayerIds());
    const roadsIncluded = layers.has("transport") && elements.exportIncludeRoads.checked;
    const treesIncluded = layers.has("lidar") && elements.exportIncludeTrees.checked;
    const canopy = selectedTreeShape() === "canopy";
    document.querySelector(".layer-height-setting").hidden = !layered;
    setControlAvailability(elements.exportBase, printable);
    setControlAvailability(elements.exportBuildingScale, true);
    setControlAvailability(elements.exportRoadWidth, printable && roadsIncluded);
    setControlAvailability(elements.exportRoadHeight, printable && roadsIncluded);
    setControlAvailability(elements.exportRoadElevation, format === "blender" && roadsIncluded);
    setControlAvailability(elements.exportTreeScale, printable && treesIncluded);
    setControlAvailability(elements.exportTreeMinHeight, printable && treesIncluded);
    setControlAvailability(elements.exportTreeTrunk, printable && treesIncluded && !canopy);
    setControlAvailability(elements.exportTreeCrown, printable && treesIncluded && !canopy);
    document.querySelectorAll('input[name="export-tree-shape"]').forEach((input) => {
      input.disabled = !treesIncluded;
    });
    const treeFieldset = document.querySelector(".tree-shape-fieldset");
    if (treeFieldset) {
      treeFieldset.hidden = !treesIncluded;
    }
    const forestFieldset = document.querySelector(".forest-quality-fieldset");
    if (forestFieldset) {
      forestFieldset.hidden = !treesIncluded || !canopy;
    }
    setControlAvailability(
      elements.exportCarveForestRoads,
      treesIncluded && canopy && layers.has("transport") && elements.exportIncludeRoads.checked,
    );
    const forestMaterialFieldset = document.querySelector(".forest-material-fieldset");
    if (forestMaterialFieldset) {
      forestMaterialFieldset.hidden = format !== "blender" || !treesIncluded || !canopy;
    }
    const forestRamp = document.querySelector(".forest-ramp-colors");
    if (forestRamp) {
      forestRamp.hidden = selectedForestMaterial() !== "procedural";
    }
    const forestTextureInput = document.querySelector(
      'input[name="export-forest-material"][value="texture"]',
    );
    if (forestTextureInput) {
      forestTextureInput.disabled = !layers.has("orthophoto");
      forestTextureInput.closest("label")?.classList.toggle(
        "is-unavailable",
        forestTextureInput.disabled,
      );
      if (forestTextureInput.disabled && forestTextureInput.checked) {
        setRadioValue("export-forest-material", "procedural");
        if (forestRamp) forestRamp.hidden = false;
      }
    }
    elements.textureModeFieldset.hidden = format !== "blender";
    const texturedOrthophoto = format === "blender"
      && layers.has("orthophoto")
      && selectedTextureMode() !== "none";
    elements.orthophotoStorageFieldset.hidden = !texturedOrthophoto;
    document.querySelector(".orthophoto-resolution-control").hidden = selectedOrthophotoStorage() !== "atlas";
    elements.colorPaletteFieldset.hidden = format !== "blender";
    setControlAvailability(elements.exportCanopyCurves, treesIncluded && canopy);
    document.querySelector(".advanced-export-settings").hidden = false;
    updateDimensionSummary();
    elements.renderFormatNote.textContent = printable
      ? "Les réglages affichés concernent uniquement les objets inclus dans ce STL."
      : "Blender conserve les hauteurs mesurées. Seuls les réglages utiles à la scène sont affichés.";
  }

  /** Cache un réglage inutile sans le vider afin de préserver un profil réutilisable. */
  function setControlAvailability(input, available) {
    input.disabled = !available;
    const label = input.closest("label");
    if (label) {
      label.hidden = !available;
    }
  }

  /** Adapte l'extension suggérée lorsque le format change. */
  function updateOutputName(force = false) {
    const current = elements.exportOutputName.value.replace(/\.(stl|blend)$/i, "");
    if (state.outputNameTouched && current) {
      elements.exportOutputName.value = current;
      return;
    }
    if (!force && current && current === state.suggestedOutputName) {
      return;
    }
    const area = selectedExportArea();
    const latitude = Number(area?.center?.[0]);
    const longitude = Number(area?.center?.[1]);
    const size = Math.round(Number(area?.size_m) || 0);
    const layerPart = selectedFormat() === "stl_layers"
      ? `couche-${layerHeightFileToken()}`
      : null;
    const sourceName = selectedSource()?.ready
      ? slugifyFilePart(selectedSource()?.label)
      : "";
    if (sourceName && size > 0) {
      elements.exportOutputName.value = [
        "export",
        sourceName,
        `${size}m`,
        layerPart,
        randomSuffix(),
      ].filter(Boolean).join("_");
      state.suggestedOutputName = elements.exportOutputName.value;
      return;
    }
    if (!Number.isFinite(latitude) || !Number.isFinite(longitude) || size <= 0) {
      elements.exportOutputName.value = ["export", layerPart, randomSuffix()]
        .filter(Boolean)
        .join("_");
      state.suggestedOutputName = elements.exportOutputName.value;
      return;
    }
    elements.exportOutputName.value = [
      "export",
      latitude.toFixed(3),
      longitude.toFixed(3),
      `${size}m`,
      layerPart,
      randomSuffix(),
    ].filter(Boolean).join("_");
    state.suggestedOutputName = elements.exportOutputName.value;
  }

  /** Rend un nom de zone sûr pour Windows tout en le gardant reconnaissable. */
  function slugifyFilePart(value) {
    return String(value || "")
      .normalize("NFD")
      .replace(/[\u0300-\u036f]/g, "")
      .replace(/[^a-zA-Z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "")
      .slice(0, 48);
  }

  /** Produit huit caractères aléatoires sans dépendance externe. */
  function randomSuffix() {
    if (window.crypto?.getRandomValues) {
      const value = new Uint32Array(1);
      window.crypto.getRandomValues(value);
      return value[0].toString(16).padStart(8, "0");
    }
    return Math.floor(Math.random() * 0xffffffff).toString(16).padStart(8, "0");
  }

  /** Construit un résumé compact des choix avant le lancement. */
  function renderReview() {
    const source = selectedSource();
    const area = selectedExportArea();
    const layers = selectedLayerIds();
    const format = selectedFormat();
    const values = [
      ["Zone", source?.label || "Non choisie"],
      ["Emprise exportée", area ? `${formatDistance(area.size_m)} × ${formatDistance(area.size_m)}` : "Non définie"],
      ["Format", formatLabel(format)],
      ["Qualité", qualityLabel(selectedQuality())],
      ["Couches", `${layers.length} sélectionnée${layers.length > 1 ? "s" : ""}`],
    ];
    if (format === "stl_layers") {
      values.push(["Hauteur des strates", `${formatNumber(Number(elements.exportLayerHeight.value))} m`]);
    }
    const objects = [];
    if (layers.includes("buildings") && elements.exportIncludeBuildings.checked) {
      objects.push("bâtiments");
      values.push([
        "Hauteur bâtiments",
        `× ${formatNumber(Number(elements.exportBuildingScale.value))}`,
      ]);
    }
    if (layers.includes("transport") && elements.exportIncludeRoads.checked) {
      objects.push("routes");
    }
    if (layers.includes("lidar") && elements.exportIncludeTrees.checked) {
      objects.push("arbres");
    }
    values.push(["Objets 3D", objects.length ? objects.join(", ") : "Aucun"]);
    if (format !== "blender") {
      values.push(["Modèle", `${elements.exportModelWidth.value} mm`]);
      if (layers.includes("transport") && elements.exportIncludeRoads.checked) {
        values.push(["Routes", `${formatNumber(Number(elements.exportRoadWidth.value))} mm minimum`]);
      }
      if (layers.includes("lidar") && elements.exportIncludeTrees.checked) {
        values.push([
          "Arbres",
          `${treeShapeLabel(selectedTreeShape())} · hauteur × ${formatNumber(Number(elements.exportTreeScale.value))}`,
        ]);
      }
    } else {
      values.push(["Textures", textureModeLabel(selectedTextureMode())]);
      if (selectedTextureMode() !== "none") {
        values.push([
          "Stockage orthophoto",
          selectedOrthophotoStorage() === "atlas"
            ? `Atlas ${Number(elements.exportOrthophotoResolution.value).toLocaleString("fr-FR")} px`
            : "Tuiles natives, qualité maximale",
        ]);
      }
      values.push([
        "Matières procédurales",
        elements.exportProceduralMaterials.checked ? "Variations discrètes" : "Couleurs unies",
      ]);
      if (layers.includes("lidar") && elements.exportIncludeTrees.checked) {
        values.push(["Arbres", treeShapeLabel(selectedTreeShape())]);
      }
    }
    elements.exportReview.replaceChildren();
    for (const [label, value] of values) {
      const item = document.createElement("div");
      item.className = "review-item";
      const key = document.createElement("span");
      key.textContent = label;
      const content = document.createElement("strong");
      content.textContent = value;
      item.append(key, content);
      elements.exportReview.append(item);
    }
  }

  /** Bloque le lancement d'une sélection qui n'est pas encore téléchargée. */
  function updateSubmitAvailability() {
    const ready = Boolean(selectedSource()?.ready) && selectedLayerIds().length > 0;
    elements.exportSubmit.disabled = !ready;
    elements.exportSubmit.title = ready
      ? ""
      : "Téléchargez d’abord cette zone avant de lancer son export.";
  }

  /** Sauvegarde éventuellement le profil puis envoie le job d'export. */
  async function submitExport(event) {
    event.preventDefault();
    const source = selectedSource();
    const layers = selectedLayerIds();
    if (!source?.ready || !layers.length) {
      dependencies.showToast("Cette zone doit être complètement téléchargée avant l’export.", true);
      return;
    }
    elements.exportSubmit.disabled = true;
    try {
      const format = selectedFormat();
      const advanced = advancedValues();
      const presetName = elements.exportPresetName.value.trim();
      if (presetName) {
        await dependencies.fetchJson("/api/v1/export-presets", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            name: presetName,
            format,
            layers,
            quality: selectedQuality(),
            advanced,
          }),
        });
      }
      await dependencies.fetchJson("/api/v1/exports", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          area: selectedExportArea(),
          format,
          layers,
          quality: selectedQuality(),
          output_name: resolvedOutputName(format),
          advanced,
          force: state.forceExport,
        }),
      });
      state.forceExport = false;
      saveLastSettings(format, layers, advanced);
      dependencies.showToast("Export ajouté à la file de travail.");
      dependencies.closePanel();
      await dependencies.refreshData();
    } catch (error) {
      dependencies.showToast(normalizeError(error), true);
      updateSubmitAvailability();
    }
  }

  /** Regroupe les réglages avancés avec les noms attendus par l'API Rust. */
  function advancedValues() {
    const format = selectedFormat();
    const layers = new Set(selectedLayerIds());
    const values = {
      vertical_exaggeration: Number(elements.exportBuildingScale.value),
      terrain_grid_step_m: Number(elements.exportTerrainGrid.value),
      include_buildings: elements.exportIncludeBuildings.checked,
      include_roads: elements.exportIncludeRoads.checked,
      include_trees: elements.exportIncludeTrees.checked,
    };
    if (format === "blender") {
      values.texture_mode = selectedTextureMode();
      values.orthophoto_storage = selectedOrthophotoStorage();
      values.orthophoto_resolution_px = Number(elements.exportOrthophotoResolution.value);
      values.procedural_materials = elements.exportProceduralMaterials.checked;
      if (layers.has("transport") && elements.exportIncludeRoads.checked) {
        values.road_elevation_m = Number(elements.exportRoadElevation.value);
      }
      values.colors = currentColorsAsRgba();
      if (layers.has("lidar") && elements.exportIncludeTrees.checked) {
        values.tree_shape = selectedTreeShape();
        if (selectedTreeShape() === "canopy") {
          values.forest_quality = selectedForestQuality();
          values.canopy_smoothing = selectedCanopySmoothing();
          values.forest_material_mode = selectedForestMaterial();
          values.carve_forest_roads = elements.exportCarveForestRoads.checked;
          values.keep_isolated_trees = elements.exportKeepIsolatedTrees.checked;
          values.colors.forest_low = hexToRgba(elements.exportForestLowColor.value);
          values.colors.forest_high = hexToRgba(elements.exportForestHighColor.value);
        }
      }
      return values;
    }
    values.width_mm = Number(elements.exportModelWidth.value);
    values.base_mm = Number(elements.exportBase.value);
    if (format === "stl_layers") {
      values.layer_height_m = Number(elements.exportLayerHeight.value);
    }
    if (layers.has("transport") && elements.exportIncludeRoads.checked) {
      values.road_min_width_mm = Number(elements.exportRoadWidth.value);
      values.road_height_mm = Number(elements.exportRoadHeight.value);
    }
    if (layers.has("lidar") && elements.exportIncludeTrees.checked) {
      values.tree_min_height_mm = Number(elements.exportTreeMinHeight.value);
      values.tree_height_scale = Number(elements.exportTreeScale.value);
      values.tree_shape = selectedTreeShape();
      if (selectedTreeShape() === "canopy") {
        values.forest_quality = selectedForestQuality();
        values.canopy_smoothing = selectedCanopySmoothing();
        values.carve_forest_roads = elements.exportCarveForestRoads.checked;
      }
      if (selectedTreeShape() !== "canopy") {
        values.tree_trunk_diameter_mm = Number(elements.exportTreeTrunk.value);
        values.tree_crown_diameter_mm = Number(elements.exportTreeCrown.value);
      }
    }
    return values;
  }

  /** Ajoute l'extension obligatoire sans accepter de chemin utilisateur. */
  function resolvedOutputName(format) {
    const extension = format === "blender" ? "blend" : "stl";
    let stem = elements.exportOutputName.value
      .trim()
      .replace(/\.(stl|blend)$/i, "")
      .replace(/[^a-zA-Z0-9À-ÿ._-]+/g, "-")
      .replace(/^[-.]+|[-.]+$/g, "") || (format === "blender" ? "scene" : "modele");
    if (format === "stl_layers" && !stem.includes("couche-")) {
      stem = `${stem}_couche-${layerHeightFileToken()}`;
    }
    return `${stem}.${extension}`;
  }

  /** Formate la hauteur de strate sans virgule ni point ambigu dans le nom de fichier. */
  function layerHeightFileToken() {
    const height = Number(elements.exportLayerHeight.value) || 1;
    return `${height.toFixed(2).replace(/0+$/, "").replace(/\.$/, "").replace(".", "p")}m`;
  }

  /** Retourne la zone actuellement choisie. */
  function selectedSource() {
    return state.sources.find((source) => source.id === state.sourceId);
  }

  /** Retourne la sous-zone courante, ou l'emprise source pour un simple aperçu. */
  function selectedExportArea() {
    const source = selectedSource();
    if (!source) {
      return null;
    }
    return source.ready ? normalizeExportArea() : source.area;
  }

  /** Retourne les couches cochées dans l'ordre du catalogue. */
  function selectedLayerIds() {
    return [...document.querySelectorAll('input[name="export-layers"]:checked')].map(
      (input) => input.value,
    );
  }

  /** Retire uniquement les objets masqués de la liste envoyée à l'aperçu synthétique. */
  function previewLayerIds() {
    const layers = selectedLayerIds().filter((layerId) => {
      if (layerId === "buildings") {
        return elements.exportIncludeBuildings.checked;
      }
      if (layerId === "transport") {
        return elements.exportIncludeRoads.checked;
      }
      if (layerId === "lidar") {
        return elements.exportIncludeTrees.checked;
      }
      return true;
    });
    return layers.length ? layers : ["none"];
  }

  /** Retourne le format courant. */
  function selectedFormat() {
    return document.querySelector('input[name="export-format"]:checked')?.value || "stl";
  }

  /** Retourne le niveau de qualité courant. */
  function selectedQuality() {
    const gridStep = Number(elements.exportTerrainGrid?.value) || 50;
    if (gridStep <= 10) return "high";
    if (gridStep <= 40) return "standard";
    return "draft";
  }

  /** Retourne la silhouette d'arbre choisie pour l'impression. */
  function selectedTreeShape() {
    return document.querySelector('input[name="export-tree-shape"]:checked')?.value || "deciduous";
  }

  /** Retourne la finesse indépendante choisie pour la surface forestière LiDAR. */
  function selectedForestQuality() {
    return document.querySelector('input[name="export-forest-quality"]:checked')?.value
      || "standard";
  }

  /** Choisit entre la photographie aérienne et la rampe procédurale de la canopée. */
  function selectedForestMaterial() {
    return document.querySelector('input[name="export-forest-material"]:checked')?.value
      || "procedural";
  }

  /** Retourne le niveau de plaquage choisi pour la scène Blender. */
  function selectedTextureMode() {
    return document.querySelector('input[name="export-texture-mode"]:checked')?.value
      || "ground_and_roofs";
  }

  /** Choisit entre les milliers de tuiles sources et une image atlas allégée. */
  function selectedOrthophotoStorage() {
    return document.querySelector('input[name="export-orthophoto-storage"]:checked')?.value
      || "atlas";
  }

  /** Déduit la courbure du niveau de détail, sauf désactivation avancée explicite. */
  function selectedCanopySmoothing() {
    if (!elements.exportCanopyCurves.checked) return 0;
    return { draft: 0.65, standard: 0.82, high: 1 }[selectedForestQuality()] || 0.82;
  }

  /** Retourne les couleurs hexadécimales directement consommables par l'aperçu Three.js. */
  function currentColors() {
    return {
      ground: elements.exportGroundColor.value,
      walls: elements.exportWallColor.value,
      roofs: elements.exportRoofColor.value,
      trees: elements.exportTreeColor.value,
      roads: elements.exportRoadColor.value,
      water: elements.exportWaterColor.value,
    };
  }

  /** Convertit la palette HTML en tableaux RGBA attendus par l'API. */
  function currentColorsAsRgba() {
    return Object.fromEntries(
      Object.entries(currentColors()).map(([name, value]) => [name, hexToRgba(value)]),
    );
  }

  /** Transforme une couleur HTML #RRGGBB en composantes normalisées. */
  function hexToRgba(value) {
    const hex = value.replace("#", "");
    return [0, 2, 4]
      .map((offset) => Number.parseInt(hex.slice(offset, offset + 2), 16) / 255)
      .concat(1);
  }

  /** Transforme une couleur RGBA sauvegardée en valeur comprise par un sélecteur HTML. */
  function rgbaToHex(value) {
    if (!Array.isArray(value) || value.length < 3) {
      return null;
    }
    const rgb = value
      .slice(0, 3)
      .map((part) => Math.round(part * 255).toString(16).padStart(2, "0"))
      .join("");
    return `#${rgb}`;
  }

  /** Produit un nombre court dans le format français. */
  function formatNumber(value) {
    return value.toLocaleString("fr-FR", { minimumFractionDigits: 1, maximumFractionDigits: 1 });
  }

  /** Produit une distance de côté compréhensible. */
  function formatDistance(metres) {
    return metres >= 1000 ? `${metres / 1000} km` : `${metres} m`;
  }

  /** Traduit les valeurs d'API en libellés courts. */
  function formatLabel(format) {
    return { stl: "STL", stl_layers: "STL par couches", blender: "Blender" }[format] || format;
  }

  /** Traduit les niveaux de qualité. */
  function qualityLabel(quality) {
    return { draft: "Rapide", standard: "Standard", high: "Haute" }[quality] || quality;
  }

  /** Traduit la silhouette d'arbre en vocabulaire utilisateur. */
  function treeShapeLabel(shape) {
    if (shape === "canopy") {
      return "Forêt continue LiDAR";
    }
    return shape === "conifer" ? "Sapin" : "Feuillu";
  }

  /** Traduit le mode de texture dans le résumé final. */
  function textureModeLabel(mode) {
    return {
      none: "Aucune, couleurs simples",
      ground: "Orthophoto sur le sol",
      ground_and_roofs: "Orthophoto sur le sol et les toits",
    }[mode] || mode;
  }

  /** Réduit une erreur réseau ou API à un message affichable. */
  function normalizeError(error) {
    return error instanceof Error ? error.message : String(error);
  }

  return { initialize, open, close, refreshSources, selectSource };
})();
