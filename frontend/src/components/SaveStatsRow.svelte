<script lang="ts">
  import type { SaveStats } from '../lib/api';
  import { formatDecimal, formatHearts, formatInteger, formatPlayTime } from '../lib/format';

  let { stats }: { stats: SaveStats | null } = $props();

  const staminaPercent = $derived(
    stats && stats.stamina.max > 0
      ? Math.round((stats.stamina.current / stats.stamina.max) * 100)
      : null,
  );

  const location = $derived(
    stats?.location.map
      ? `${stats.location.map}${stats.location.map_type ? ` · ${stats.location.map_type}` : ''}`
      : '—',
  );

  function hint(text: string): string {
    return stats ? text : 'No save loaded';
  }
</script>

<section class="stats-strip" aria-label="Save statistics">
  <div class="stats-strip-item">
    <span class="stat-label">Play time</span>
    <strong class="stats-strip-value">
      {stats ? formatPlayTime(stats.play_time_seconds) : '—'}
    </strong>
    <span class="stat-hint">{hint('from the play report')}</span>
  </div>

  <div class="stats-strip-item">
    <span class="stat-label">Rupees</span>
    <strong class="stats-strip-value">{stats ? formatInteger(stats.rupees) : '—'}</strong>
    <span class="stat-hint">{hint('current wallet')}</span>
  </div>

  <div class="stats-strip-item">
    <span class="stat-label">Hearts</span>
    <strong class="stats-strip-value">
      {stats ? formatHearts(stats.hearts.current, stats.hearts.max) : '—'}
    </strong>
    <span class="stat-hint">{hint('heart containers')}</span>
  </div>

  <div class="stats-strip-item">
    <span class="stat-label">Stamina</span>
    <strong class="stats-strip-value">
      {stats
        ? `${formatDecimal(stats.stamina.current)} / ${formatInteger(stats.stamina.max)}`
        : '—'}
    </strong>
    <span class="stat-hint">
      {staminaPercent !== null ? `${staminaPercent}% of capacity` : hint('current stamina')}
    </span>
  </div>

  <div class="stats-strip-item">
    <span class="stat-label">Location</span>
    <strong class="stats-strip-value">{location}</strong>
    <span class="stat-hint">{hint('last saved position')}</span>
  </div>
</section>

<style>
  .stats-strip {
    display: grid;
    grid-template-columns: repeat(5, minmax(0, 1fr));
    gap: 12px;
    margin-bottom: 22px;
    padding: 17px 19px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: linear-gradient(180deg, rgba(15, 37, 48, 0.7), rgba(9, 26, 33, 0.65));
  }

  .stats-strip-item {
    min-width: 0;
  }

  .stats-strip-value {
    display: block;
    margin-top: 7px;
    font-family: Georgia, 'Times New Roman', serif;
    font-size: 19px;
    font-weight: 500;
    line-height: 1.1;
    color: var(--text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  @media (min-width: 481px) and (max-width: 1040px) {
    .stats-strip {
      grid-template-columns: repeat(3, minmax(0, 1fr));
    }
  }

  @media (max-width: 480px) {
    .stats-strip {
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 14px;
      padding: 15px;
    }
  }
</style>
