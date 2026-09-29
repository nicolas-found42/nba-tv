# NBA TV Archive

Personal archive of NBA games: the schedule and result of each game, with video where video exists.

## Language

### Archive

**Season**:
One year of NBA games, from first game to Finals.
_Avoid_: year

**Team**:
One NBA club, active or defunct.

**Game**:
One contest between two teams on one date. Covers NBA/BAA games only; ABA games are out of scope.

**Schedule**:
The full list of games in one season. One team's games are a view over it.

**Box Score**:
The recorded result of one game: team totals and player totals.
_Avoid_: stats, stat line

**Game Tape**:
The full video record of one game, when it exists.
_Avoid_: footage, stream, video

### Sources

**Tape Source**:
One place that holds game tape: official archive, fan channel, or file library.

**Source Ladder**:
The fixed order in which tape sources are searched for one game.

**Rung**:
One step of the Source Ladder, numbered 0–7. Each rung has a playback class: rungs 1 and 4 are progressive files (Cache Tier-eligible), rungs 0, 2 and 3 are External Surfaces, rungs 5–7 are pointers only (existence metadata, never playable).
_Avoid_: tier, level

**Sweep**:
One pass over a game's rungs 0–4 in ladder order, recording what each rung found. A game whose rungs are not all consumed is still sweeping; one whose rungs are all consumed with no likely match is unavailable.
_Avoid_: scan, crawl (a crawl fetches schedule and box score pages, not tape)

**Probe**:
The per-rung lookup a sweep runs for one game (`SourceProbe`): it returns the query it used plus candidate tape sources as evidence, and never scores them.
_Avoid_: scraper, checker

**External Surface**:
A tape source that plays in a vendor player, not in the player backend. It can sit in the shell or outside it.

**Cache Tier**:
The driver's personal file store for downloadable tape copies.
_Avoid_: Drive, cloud, host

**Drive mirror**:
The optional stage of the Cache Tier that copies ready cache entries to the driver's Drive account with `rclone copy` (copy-only, never sync, move or delete). The word "Drive" is allowed for this stage only; it is still avoided for the Cache Tier itself.
_Avoid_: backup, sync

### App

**Shell**:
The app window and all browse screens in it.

**Player Backend**:
The part that turns tape bytes into moving pictures.

**Lane A**:
The Player Backend lane that decodes progressive-file tape through the `ffmpeg` sidecar into frames the Shell draws in its own window.

**Lane B**:
The Player Backend lane that hosts a vendor's sanctioned embed player in a webview inside the Shell. Plays External Surfaces; the app never touches the bytes.
