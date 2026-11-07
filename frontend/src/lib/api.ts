export type DeploymentMode = 'local' | 'lan' | 'hub';

export interface AppConfig {
  mode: DeploymentMode;
  version: string;
  live: boolean;
  save_dir: string | null;
  control_addr: string | null;
  map_assets: boolean;
}

export async function getConfig(): Promise<AppConfig> {
  const response = await fetch('/api/config', {
    headers: { accept: 'application/json' },
  });
  if (!response.ok) {
    throw new Error(`Server returned ${response.status}`);
  }
  return (await response.json()) as AppConfig;
}
