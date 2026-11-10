<script lang="ts">
  import {
    deleteProfileSlot,
    getProfileSlots,
    getProfiles,
    importPath,
    uploadSlot,
    type AppConfig,
    type ImportResult,
    type Profile,
    type SlotSummary,
    type User,
  } from '../lib/api';
  import { formatSavedAt } from '../lib/format';
  import { parseSlotIndex } from '../lib/slotIndex';

  let {
    config,
    user,
    onchanged,
  }: {
    config: AppConfig | null;
    user: User | null;
    onchanged: () => void;
  } = $props();

  let profiles = $state<Profile[]>([]);
  let profileId = $state<string | null>(null);
  let slots = $state<SlotSummary[]>([]);
  let profileError = $state<string | null>(null);
  let slotsError = $state<string | null>(null);
  let slotsLoading = $state(false);
  let request = 0;

  let gameData = $state<File | null>(null);
  let captionSav = $state<File | null>(null);
  let captionImage = $state<File | null>(null);
  let slotIndexText = $state('');
  let uploading = $state(false);
  let uploadError = $state<string | null>(null);
  let uploadNotice = $state<string | null>(null);

  let importDir = $state('');
  let importing = $state(false);
  let importError = $state<string | null>(null);
  let importResult = $state<ImportResult | null>(null);
  let deletingSlot = $state<number | null>(null);

  const isLocal = $derived(config?.mode === 'local');
  const importSummary = $derived.by(() => {
    const result = importResult;
    if (result === null) return '';
    const noun = result.imported.length === 1 ? 'slot' : 'slots';
    return result.errors.length > 0
      ? `Imported ${result.imported.length} ${noun}; ${result.errors.length} failed.`
      : `Imported ${result.imported.length} ${noun}.`;
  });

  // (Re)load profiles whenever the signed-in user changes.
  $effect(() => {
    if (user === null) {
      profiles = [];
      profileId = null;
      slots = [];
      profileError = null;
      return;
    }
    void loadProfiles();
  });

  async function loadProfiles() {
    profileError = null;
    try {
      const loaded = await getProfiles();
      profiles = loaded;
      if (profileId === null || !loaded.some((profile) => profile.id === profileId)) {
        profileId = loaded[0]?.id ?? null;
      }
    } catch (cause) {
      profileError = cause instanceof Error ? cause.message : String(cause);
      profiles = [];
      profileId = null;
    }
  }

  // Reload the stored slots whenever the selected profile changes.
  $effect(() => {
    const id = profileId;
    if (id === null) {
      slots = [];
      return;
    }
    void loadSlots(id);
  });

  async function loadSlots(id: string) {
    const current = ++request;
    slotsLoading = true;
    slotsError = null;
    try {
      const loaded = await getProfileSlots(id);
      if (current !== request) return;
      slots = loaded;
    } catch (cause) {
      if (current !== request) return;
      slots = [];
      slotsError = cause instanceof Error ? cause.message : String(cause);
    } finally {
      if (current === request) slotsLoading = false;
    }
  }

  function readFile(event: Event): File | null {
    return (event.currentTarget as HTMLInputElement).files?.[0] ?? null;
  }

  function resetUpload() {
    gameData = null;
    captionSav = null;
    captionImage = null;
    slotIndexText = '';
  }

  async function submitUpload(event: SubmitEvent) {
    event.preventDefault();
    if (uploading) return;

    uploadError = null;
    uploadNotice = null;

    if (profileId === null) {
      uploadError = 'No profile available to upload into.';
      return;
    }
    if (gameData === null) {
      uploadError = 'Choose a game_data.sav file first.';
      return;
    }

    const parsed = parseSlotIndex(slotIndexText);
    if (!parsed.ok) {
      uploadError = parsed.error;
      return;
    }
    const slotIndex = parsed.value;

    const profile = profileId;
    uploading = true;
    try {
      const saved = await uploadSlot(
        profile,
        { game_data: gameData, caption_sav: captionSav, caption_image: captionImage },
        slotIndex,
      );
      uploadNotice = `Stored "${saved.label}" in slot ${saved.slot}.`;
      resetUpload();
      await loadSlots(profile);
      onchanged();
    } catch (cause) {
      uploadError = cause instanceof Error ? cause.message : String(cause);
    } finally {
      uploading = false;
    }
  }

  async function deleteSlot(slot: number) {
    const id = profileId;
    if (id === null || deletingSlot !== null) return;
    if (!window.confirm(`Delete slot #${slot} and its stored snapshots? This cannot be undone.`)) {
      return;
    }

    deletingSlot = slot;
    slotsError = null;
    try {
      await deleteProfileSlot(id, slot);
      await loadSlots(id);
      onchanged();
    } catch (cause) {
      slotsError = cause instanceof Error ? cause.message : String(cause);
    } finally {
      deletingSlot = null;
    }
  }

  async function submitImport(event: SubmitEvent) {
    event.preventDefault();
    if (importing || importDir.trim() === '') return;

    importError = null;
    importResult = null;
    importing = true;
    try {
      importResult = await importPath(importDir.trim());
      onchanged();
    } catch (cause) {
      importError = cause instanceof Error ? cause.message : String(cause);
    } finally {
      importing = false;
    }
  }
</script>

<div class="page">
  <section class="section">
    <div class="section-head">
      <div>
        <span class="eyebrow">Saved data</span>
        <h2>Save slots</h2>
        <p>
          {#if user !== null}
            Upload a save into one of your profiles and review what was stored.
          {:else}
            Sign in to upload saves into a profile.
          {/if}
        </p>
      </div>
    </div>

    {#if user === null}
      <p class="saves-hint">
        Uploads belong to a signed-in account. In local mode the save folder is
        ingested automatically, and you can import an existing folder below.
      </p>
    {:else}
      {#if profileError !== null}
        <p class="saves-error" role="alert">{profileError}</p>
      {/if}

      <div class="profile-row">
        <label class="field">
          <span class="field-label">Profile</span>
          <select bind:value={profileId} disabled={profiles.length === 0}>
            {#each profiles as profile (profile.id)}
              <option value={profile.id}>{profile.name}</option>
            {/each}
          </select>
        </label>
        <button type="button" class="ghost-button" onclick={() => void loadProfiles()}>
          Refresh
        </button>
      </div>

      {#if slotsError !== null}
        <p class="saves-error" role="alert">{slotsError}</p>
      {:else if slotsLoading}
        <p class="saves-hint">Loading stored slots...</p>
      {:else if slots.length === 0}
        <p class="saves-hint">No saves stored for this profile yet.</p>
      {:else}
        <ul class="slot-list">
          {#each slots as slot (slot.slot)}
            <li class="slot-item">
              <span class="slot-index">#{slot.slot}</span>
              <span class="slot-body">
                <span class="slot-label">{slot.label}</span>
                <span class="slot-meta">{formatSavedAt(slot.saved_at) || '—'}</span>
              </span>
              <button
                type="button"
                class="slot-delete"
                disabled={deletingSlot !== null}
                onclick={() => void deleteSlot(slot.slot)}
              >
                {deletingSlot === slot.slot ? 'Deleting…' : 'Delete'}
              </button>
            </li>
          {/each}
        </ul>
      {/if}

      <form class="saves-form" onsubmit={submitUpload}>
        <h3 class="form-title">Upload a save</h3>
        <label class="field">
          <span class="field-label">game_data.sav <em>(required)</em></span>
          <input type="file" accept=".sav,application/octet-stream" onchange={(event) => (gameData = readFile(event))} />
        </label>
        <label class="field">
          <span class="field-label">caption.sav <em>(optional)</em></span>
          <input type="file" accept=".sav" onchange={(event) => (captionSav = readFile(event))} />
        </label>
        <label class="field">
          <span class="field-label">caption.jpg <em>(optional)</em></span>
          <input type="file" accept="image/jpeg" onchange={(event) => (captionImage = readFile(event))} />
        </label>
        <label class="field slot-number">
          <span class="field-label">Slot number <em>(optional)</em></span>
          <input
            type="text"
            inputmode="numeric"
            pattern="[0-9]*"
            placeholder="next free"
            bind:value={slotIndexText}
          />
        </label>

        {#if uploadError !== null}
          <p class="saves-error" role="alert">{uploadError}</p>
        {/if}
        {#if uploadNotice !== null}
          <p class="saves-ok" role="status">{uploadNotice}</p>
        {/if}

        <button class="primary-button" type="submit" disabled={uploading}>
          {uploading ? 'Uploading...' : 'Upload save'}
        </button>
      </form>
    {/if}
  </section>

  {#if isLocal}
    <section class="section">
      <div class="section-head">
        <div>
          <span class="eyebrow">Local import</span>
          <h2>Import a save folder</h2>
          <p>Ingest an existing Savage Wind save folder into this machine's store.</p>
        </div>
      </div>

      {#if config?.save_dir}
        <p class="saves-hint">
          Watched folder: <code class="inline-code">{config.save_dir}</code>
        </p>
      {/if}

      <form class="saves-form" onsubmit={submitImport}>
        <label class="field">
          <span class="field-label">Save folder path</span>
          <input bind:value={importDir} placeholder="/home/you/.local/share/savagewind/save" />
        </label>

        {#if importError !== null}
          <p class="saves-error" role="alert">{importError}</p>
        {/if}
        {#if importResult !== null}
          <p class="saves-ok" role="status">{importSummary}</p>
          {#if importResult.errors.length > 0}
            <ul class="slot-list">
              {#each importResult.errors as item (item.slot)}
                <li class="slot-item">
                  <span class="slot-index">#{item.slot}</span>
                  <span class="slot-body">
                    <span class="slot-meta">{item.error}</span>
                  </span>
                </li>
              {/each}
            </ul>
          {/if}
        {/if}

        <button class="primary-button" type="submit" disabled={importing || importDir.trim() === ''}>
          {importing ? 'Importing...' : 'Import folder'}
        </button>
      </form>
    </section>
  {/if}
</div>

<style>
  .profile-row {
    display: flex;
    align-items: flex-end;
    gap: 12px;
    margin-bottom: 18px;
  }

  .profile-row .field {
    flex: 1;
    max-width: 360px;
  }

  .slot-list {
    margin: 0 0 20px;
    padding: 0;
    list-style: none;
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    overflow: hidden;
  }

  .slot-item {
    display: flex;
    align-items: center;
    gap: 13px;
    padding: 11px 14px;
    border-top: 1px solid rgba(27, 57, 67, 0.6);
    background: rgba(9, 26, 33, 0.4);
  }

  .slot-item:first-child {
    border-top: 0;
  }

  .slot-index {
    flex: none;
    width: 42px;
    color: var(--cyan);
    font-size: 13px;
    font-variant-numeric: tabular-nums;
  }

  .slot-body {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .slot-label {
    font-size: 13.5px;
  }

  .slot-meta {
    color: var(--muted);
    font-size: 11.5px;
    overflow-wrap: anywhere;
  }

  .slot-delete {
    flex: none;
    align-self: center;
    padding: 4px 10px;
    font-size: 11.5px;
    color: var(--muted);
    background: none;
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    cursor: pointer;
  }

  .slot-delete:hover:not(:disabled) {
    color: var(--danger);
    border-color: var(--danger);
  }

  .slot-delete:disabled {
    cursor: default;
    opacity: 0.6;
  }

  .saves-form {
    display: flex;
    flex-direction: column;
    gap: 14px;
    margin-top: 18px;
    padding-top: 18px;
    border-top: 1px solid var(--line);
  }

  .form-title {
    font-size: 16px;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .field-label {
    color: var(--muted);
    font-size: 11px;
    letter-spacing: 0.13em;
    text-transform: uppercase;
  }

  .field-label em {
    color: var(--muted);
    font-style: normal;
    letter-spacing: 0;
    text-transform: none;
  }

  .field input,
  .field select {
    padding: 10px 12px;
    border: 1px solid var(--line-bright);
    border-radius: var(--radius-sm);
    background: var(--panel-solid);
    color: var(--text);
    font-size: 14px;
  }

  .field input:focus,
  .field select:focus {
    outline: none;
    border-color: var(--cyan-dim);
    box-shadow: 0 0 0 3px rgba(93, 219, 234, 0.14);
  }

  .slot-number {
    max-width: 200px;
  }

  .saves-hint {
    color: var(--muted);
    font-size: 12.5px;
    line-height: 1.6;
  }

  .saves-error {
    padding: 9px 12px;
    border: 1px solid rgba(240, 139, 139, 0.4);
    border-radius: var(--radius-sm);
    background: rgba(50, 19, 22, 0.5);
    color: #ffd9d9;
    font-size: 12.5px;
  }

  .saves-ok {
    padding: 9px 12px;
    border: 1px solid rgba(120, 217, 166, 0.35);
    border-radius: var(--radius-sm);
    background: rgba(20, 50, 38, 0.5);
    color: #c9f2dd;
    font-size: 12.5px;
  }

  .primary-button,
  .ghost-button {
    align-self: flex-start;
    padding: 10px 14px;
    border-radius: var(--radius-sm);
    font-size: 13.5px;
    cursor: pointer;
    transition:
      border-color 140ms ease,
      background 140ms ease,
      opacity 140ms ease;
  }

  .primary-button {
    border: 1px solid rgba(93, 219, 234, 0.35);
    background: linear-gradient(135deg, rgba(93, 219, 234, 0.22), rgba(93, 219, 234, 0.08));
    color: var(--text);
  }

  .primary-button:hover:not(:disabled) {
    border-color: rgba(93, 219, 234, 0.55);
    background: linear-gradient(135deg, rgba(93, 219, 234, 0.3), rgba(93, 219, 234, 0.12));
  }

  .ghost-button {
    border: 1px solid var(--line-bright);
    background: transparent;
    color: var(--text-soft);
  }

  .ghost-button:hover {
    color: var(--text);
    border-color: var(--cyan-dim);
  }

  .primary-button:disabled,
  .ghost-button:disabled {
    opacity: 0.55;
    cursor: not-allowed;
  }
</style>
