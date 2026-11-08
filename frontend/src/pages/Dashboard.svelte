<script lang="ts">
  import type { AppConfig, CompletionSummary, SlotDetail, SlotSummary } from '../lib/api';
  import SaveStatsRow from '../components/SaveStatsRow.svelte';
  import { formatMapArea, formatSavedAt } from '../lib/format';

  let {
    config,
    slots,
    summary,
    saveError,
    loading,
  }: {
    config: AppConfig | null;
    slots: SlotSummary[];
    summary: SlotDetail | null;
    saveError: string | null;
    loading: boolean;
  } = $props();

  const COMPLETION_CARDS: { label: string; key: keyof CompletionSummary }[] = [
    { label: 'Shrines', key: 'shrines' },
    { label: 'Korok seeds', key: 'koroks' },
    { label: 'Sheikah towers', key: 'towers' },
    { label: 'Recovered memories', key: 'memories' },
  ];

  let captionError = $state(false);

  $effect(() => {
    void summary?.caption_url;
    captionError = false;
  });

  const modeName = $derived(
    config === null
      ? 'Connecting…'
      : config.mode === 'hub'
        ? 'Public hub'
        : config.mode === 'lan'
          ? 'LAN server'
          : 'Local',
  );

  const saveSource = $derived(
    config === null
      ? 'Checking…'
      : config.mode === 'hub'
        ? 'Upload or agent sync'
        : config.save_dir
          ? 'Folder detected'
          : 'Not configured',
  );

  const saveDone = $derived(slots.length > 0);
  const mapDone = $derived(config?.map_assets === true);
  const liveDone = $derived(config?.live === true);

  const completionCards = $derived(
    COMPLETION_CARDS.map(({ label, key }) => {
      const count = summary?.completion[key];
      return {
        label,
        value: count ? `${count.completed}/${count.total}` : '—',
        hint: count
          ? `${count.percentage.toFixed(1)}% complete`
          : loading
            ? 'Loading save…'
            : saveError
              ? 'Save unavailable'
              : 'No save loaded',
      };
    }),
  );

  const activeLabel = $derived(
    summary?.label ?? (loading ? 'Loading save…' : saveSource),
  );

  const activeNote = $derived.by(() => {
    if (!summary) {
      return (
        config?.save_dir ??
        (config?.mode === 'hub' ? 'Uploads and agent sync' : 'Set WAYFINDER_SAVE_DIR')
      );
    }

    const area = formatMapArea(summary.map_area);
    const saved = formatSavedAt(summary.saved_at);
    return [
      area || summary.location || `Slot ${summary.slot}`,
      saved ? `saved ${saved}` : '',
    ]
      .filter(Boolean)
      .join(' · ');
  });
</script>

<div class="page">
  <section class="hero">
    <div class="hero-main">
      <span class="eyebrow">Sheikah Slate companion</span>
      <h2>Your journey through Hyrule, remembered.</h2>
      <p>
        Wayfinder Slate reads your Breath of the Wild save and turns it into a live
        atlas: completion, play history, and — when Savage Wind is running — Link's
        position on the map. No Nintendo account, no console link, no save editing.
      </p>
    </div>

    <div class="hero-side">
      <div class="mini-card">
        <h3>Deployment</h3>
        <div class="mini-value">{modeName}</div>
        <div class="mini-note">
          {config?.mode === 'hub'
            ? 'Private saves by default'
            : 'Runs beside Savage Wind'}
        </div>
      </div>
      <div class="mini-card save-source-card">
        {#if summary && !captionError}
          <img
            class="caption-thumb"
            src={summary.caption_url}
            alt={`Screenshot for ${summary.label}`}
            loading="lazy"
            onerror={() => (captionError = true)}
          />
        {/if}
        <h3>Save source</h3>
        <div class="mini-value">{activeLabel}</div>
        <div class="mini-note">{activeNote}</div>
      </div>
      <div class="mini-card">
        <h3>Live navigation</h3>
        <div class="mini-value">{liveDone ? 'Available' : 'Not connected'}</div>
        <div class="mini-note">
          {config?.mode === 'hub' ? 'Outbound agent push' : 'Savage Wind control socket'}
        </div>
      </div>
    </div>
  </section>

  {#if saveError}
    <div class="notice error" role="status" style="margin-bottom: 18px">
      <strong>Save unavailable.</strong>
      <span>{saveError}</span>
    </div>
  {/if}

  <SaveStatsRow stats={summary?.stats ?? null} />

  <section class="grid" aria-label="Completion summary">
    {#each completionCards as stat (stat.label)}
      <article class="stat-card">
        <div class="stat-label">{stat.label}</div>
        <div class="stat-value">{stat.value}</div>
        <div class="stat-hint">{stat.hint}</div>
      </article>
    {/each}
  </section>

  <div class="two-col">
    <section class="section">
      <div class="section-head">
        <div>
          <h2>First-run checklist</h2>
          <p>The pieces Wayfinder Slate needs before your save comes alive.</p>
        </div>
      </div>

      <div class="checklist">
        <div class="check" class:done={config !== null}>
          <span class="check-mark">{config !== null ? '✓' : '1'}</span>
          <div>
            <div class="check-title">Server connection</div>
            <div class="check-detail">
              {config !== null
                ? `Connected to Wayfinder Slate ${config.version}.`
                : 'Waiting for the Rust server on /api.'}
            </div>
          </div>
        </div>

        <div class="check" class:done={saveDone}>
          <span class="check-mark">{saveDone ? '✓' : '2'}</span>
          <div>
            <div class="check-title">Save folder</div>
            <div class="check-detail">
              {#if config?.mode === 'hub'}
                Uploads and agent sync arrive with the save-ingestion milestone.
              {:else if slots.length > 0}
                {slots.length} save slot{slots.length === 1 ? '' : 's'} loaded from
                <code>{config?.save_dir}</code>.
              {:else if config?.save_dir}
                Found at <code>{config?.save_dir}</code>, but no numbered slot
                contains <code class="inline-code">game_data.sav</code>.
              {:else}
                Set <code class="inline-code">WAYFINDER_SAVE_DIR</code> to Savage
                Wind's <code class="inline-code">save</code> folder.
              {/if}
            </div>
          </div>
        </div>

        <div class="check" class:done={mapDone}>
          <span class="check-mark">{mapDone ? '✓' : '3'}</span>
          <div>
            <div class="check-title">Map assets</div>
            <div class="check-detail">
              {#if mapDone}
                BotW tiles and marker data are available.
              {:else}
                Run <code class="inline-code">./scripts/fetch-map-assets.sh</code>
                to download the attributed Zelda Dungeon tiles.
              {/if}
            </div>
          </div>
        </div>

        <div class="check" class:done={liveDone}>
          <span class="check-mark">{liveDone ? '✓' : '4'}</span>
          <div>
            <div class="check-title">Live telemetry</div>
            <div class="check-detail">
              {#if config?.mode === 'hub'}
                A local <code class="inline-code">wayfinder-agent</code> will push
                telemetry outbound; the hub never connects inward.
              {:else if config?.live}
                Start Savage Wind with
                <code class="inline-code">SAVAGEWIND_CONTROL=7777</code>.
                Direct polling lands in the live-mode milestone.
              {:else}
                Live mode is unavailable in this deployment.
              {/if}
            </div>
          </div>
        </div>
      </div>
    </section>

    <section class="section">
      <div class="section-head">
        <div>
          <h2>What happens next</h2>
          <p>The current build reads saves directly; history comes later.</p>
        </div>
      </div>

      <div class="timeline">
        <div class="timeline-item">
          <span class="timeline-phase">M2</span>
          <div>
            <div class="timeline-title">Save parser</div>
            <div class="timeline-desc">
              Slot discovery, captions, player stats, and completion flags from
              1.9.0 saves.
            </div>
          </div>
        </div>
        <div class="timeline-item">
          <span class="timeline-phase">M4</span>
          <div>
            <div class="timeline-title">Save ingestion</div>
            <div class="timeline-desc">
              Folder watching, debounced snapshots, uploads, and history.
            </div>
          </div>
        </div>
        <div class="timeline-item">
          <span class="timeline-phase">M5</span>
          <div>
            <div class="timeline-title">The map</div>
            <div class="timeline-desc">
              Shrine, tower, memory, and Korok layers with selected-slot completion.
            </div>
          </div>
        </div>
        <div class="timeline-item">
          <span class="timeline-phase">M7</span>
          <div>
            <div class="timeline-title">Live Link</div>
            <div class="timeline-desc">
              Savage Wind polling locally, with an outbound agent for remote hubs.
            </div>
          </div>
        </div>
      </div>

      <div class="notice" style="margin-top: 16px">
        <strong>{summary ? 'Save connected.' : 'Early build.'}</strong>
        <span>
          {#if summary}
            Play time, rupees, hearts, stamina, completion, and the caption screenshot
            come from the selected slot. Use the picker or refresh after saving
            in-game; automatic folder watching lands with save ingestion.
          {:else}
            The UI shell, parser, and slot discovery are in place. Set
            <code class="inline-code">WAYFINDER_SAVE_DIR</code> to load a save and
            replace these placeholders.
          {/if}
        </span>
      </div>
    </section>
  </div>
</div>

<style>
  .save-source-card {
    display: flex;
    flex-direction: column;
  }

  .caption-thumb {
    width: 100%;
    max-height: 130px;
    object-fit: cover;
    margin-bottom: 10px;
    border: 1px solid var(--line-bright);
    border-radius: var(--radius-sm);
  }
</style>
