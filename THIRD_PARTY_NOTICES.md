# Third-party notices

Wayfinder Slate is an unofficial fan project. It is not affiliated with or
endorsed by Nintendo, Zelda Dungeon, or the Savage Wind project.

The MIT License in [`LICENSE.md`](LICENSE.md) applies only to Wayfinder Slate's
source code. It does not apply to any third-party game assets, map tiles, marker
data, icons, or other material described below.

## Map tiles, marker data, and map icons

- Source: <https://github.com/zeldadungeon/maps>
- Copyright: Zelda Dungeon and respective contributors.
- The repository does not declare a licence. Wayfinder Slate does not reuse the
  upstream application code; it fetches only the Breath of the Wild map tiles,
  marker JSON, and icons at a pinned commit.
- The assets are downloaded at build/setup time and are not committed to this
  repository. See `scripts/fetch-map-assets.sh`.
- The Legend of Zelda and Breath of the Wild are trademarks of Nintendo.

## Map object completion dataset (`frontend/static/map/botw/markers/map_objects.json`)

This static dataset joins Zelda Dungeon marker coordinates with save flag names
from MIT-licensed community data. It is generated offline and committed as a
build artifact.

Sources:
- **Zelda Dungeon markers** (shrines, towers, Korok seeds, memories, locations,
  enemies): <https://github.com/zeldadungeon/maps> — no licence declared, used
  with attribution.
- **Save flag names, coordinates, and object lists** (shrines, towers, Koroks,
  locations, enemies): <https://github.com/marcrobledo/savegame-editors> — MIT
  licensed (Copyright 2017–2023 Marc Robledo). Uses `zelda-botw.data.js`,
  `zelda-botw.locations.js`, `zelda-botw.hashes.csv`.
- **Shrine names by CDungeon number**: <https://github.com/MrCheeze/botw-tools>
  `text/StaticMsg/Dungeon.xmsbt` — no licence declared; used only as a factual
  reference for the 136 shrine name mappings.

The generated dataset (`map_objects.json`) is original work by the Wayfinder
Slate authors derived from the above sources.

## Save flag names

Community-maintained CRC32 flag-name data used to interpret save keys derives
from MIT-licensed Breath of the Wild save research, including
<https://github.com/marcrobledo/savegame-editors> and related community
datasets. Only derived key names and mappings are used.

## Savage Wind

- Source: <https://github.com/Revivedx/SavageWindRecomp>
- Wayfinder Slate communicates with Savage Wind's documented local control
  server using read-only, player-safe commands. No Savage Wind code is vendored.
