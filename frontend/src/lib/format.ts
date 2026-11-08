export function formatPlayTime(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds <= 0) return '0m';

  const totalMinutes = Math.floor(seconds / 60);
  const days = Math.floor(totalMinutes / 1440);
  const hours = Math.floor((totalMinutes % 1440) / 60);
  const minutes = totalMinutes % 60;
  const parts: string[] = [];

  if (days > 0) parts.push(`${days}d`);
  if (hours > 0) parts.push(`${hours}h`);
  parts.push(`${minutes}m`);

  return parts.join(' ');
}

export function formatInteger(value: number): string {
  if (!Number.isFinite(value)) return '—';
  return new Intl.NumberFormat().format(value);
}

export function formatDecimal(value: number, maximumFractionDigits = 1): string {
  if (!Number.isFinite(value)) return '—';
  return new Intl.NumberFormat(undefined, { maximumFractionDigits }).format(value);
}

export function formatHearts(current: number, max: number): string {
  const heartValue = (value: number): string => {
    if (!Number.isFinite(value)) return '—';
    return Number.isInteger(value) ? value.toFixed(1) : formatDecimal(value, 2);
  };

  return `${heartValue(current)} / ${heartValue(max)}`;
}

export function formatMapArea(value: string): string {
  if (!value) return '';
  return value
    .replace(/^MapArea_/, '')
    .replace(/([a-z0-9])([A-Z])/g, '$1 $2');
}

export function formatSavedAt(unixSeconds: number): string {
  if (!Number.isFinite(unixSeconds) || unixSeconds <= 0) return '';
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short',
  }).format(new Date(unixSeconds * 1000));
}
