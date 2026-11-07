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

## Save flag names

Community-maintained CRC32 flag-name data used to interpret save keys derives
from MIT-licensed Breath of the Wild save research, including
<https://github.com/GingerAvalanche/botw_flag_util> and related community
datasets. Only derived key names and mappings are used.

## Savage Wind

- Source: <https://github.com/Revivedx/SavageWindRecomp>
- Wayfinder Slate communicates with Savage Wind's documented local control
  server using read-only, player-safe commands. No Savage Wind code is vendored.
