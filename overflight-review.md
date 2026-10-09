# overflight: review tasks

Review of `main` at d4974f6 (v0.2.0). Build, clippy and all 84 tests pass. The items below are ordered by priority. Each one says what's wrong, how to reproduce it, what to change, and how to know it's fixed. Work through them in order, one commit per item, and keep `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` green after each.

## Bugs

### 1. Quitting can hang for up to 20 seconds

`Fetcher` joins its thread on drop, and the thread may be inside a blocking HTTP request with a 20-second timeout. The terminal is restored first, so the user sees their shell, but the prompt doesn't come back.

**Repro** (confirmed): serve an `aircraft.json` that takes 25 s to respond, run `overflight --source local --url http://127.0.0.1:8765/aircraft.json`, wait 3 s and press `q`. The process exits 17 s later.

**Fix:**
- Don't block on the join when quitting. Set the shutdown flag and let the process exit; the thread is idle-safe and holds nothing that needs flushing.
- If you'd rather keep a clean join, wait for at most ~200 ms with a channel and then move on.
- Also set a `connect_timeout` of about 5 s on the client, and lower the overall timeout to about 10 s.

**Done when:** the same repro exits in under half a second, with a test that drops a `Fetcher` whose provider blocks for a long time and checks the drop returns quickly.

### 2. Satellite data: fetched on every launch, and stale data shown as current

Two related problems in `satellite.rs` / `run_live`:

- **Every launch downloads TLEs from Celestrak.** Celestrak's usage policy says GP data updates once every 2 hours and asks clients to download only once per update. On any non-200 response, software should stop querying, and repeatedly ignoring errors gets the IP firewalled. Someone using overflight as a screensaver may start it many times an hour, so it will break this policy and could get their IP blocked.
- **The embedded TLEs are used in live mode.** `run_live` loads `satellite::embedded()` first and only replaces it if the Celestrak fetch succeeds. When the user is offline or blocked, they see satellites propagated from TLEs that are months old by the time they install the release. SGP4 error grows quickly with TLE age, so the ISS can be drawn far from where it really is, with nothing to say so.

**Fix:**
- Cache the TLE file in the platform cache dir (`ProjectDirs::cache_dir()`) with its fetch time, and only refetch when it's older than 12 hours.
- On any non-200 response or network error, write a "don't retry before" timestamp (24 h ahead) and fall back to the cache.
- In live mode, use embedded TLEs only for `--demo`. Never draw satellites from TLEs older than 7 days; say "satellite data out of date" in the status line instead.

**Done when:** tests cover the cache fresh/stale/backoff decisions with a fake clock, and launching ten times in a row makes one request.

### 3. The observer is always at sea level

`Query::observer()` uses an altitude of 0 m, and aircraft altitudes are measured from sea level. For anyone living above sea level, low aircraft are drawn far too high in the sky. Example near Denver (1,650 m): a plane on approach at 1,830 m, 5 km away, is really about 2° above the horizon, but overflight draws it at about 20°. Helicopters and approaches are exactly the low, close traffic people notice.

There's a second, smaller problem: aircraft without an altitude are placed at 0 m and quietly fall out because they end up just below the horizon, and ground traffic is only filtered out by the same accident. That will stop working once the observer has a real altitude.

**Fix:**
- Add an `alt_m` setting (observer height above sea level), configurable from the config file and `--alt`, and use it in `Query::observer()`.
- Explicitly skip `on_ground` aircraft and aircraft without an altitude in `App::apply`, rather than relying on the horizon to hide them.
- Document in a comment that `alt_baro` is pressure altitude (close to height above sea level) and `alt_geom` is GNSS height; the difference doesn't matter at this scale.

**Done when:** a test with the Denver numbers gives about 2°, and ground and no-altitude aircraft are skipped by a test, not by accident.

### 4. No validation of location and radius

`--lat 200`, `--lon -500`, `--radius-km 0`, `--radius-km -50` and NaN are all accepted, from the CLI and from the config file.
- With OpenSky, a negative radius builds an inverted bounding box, which the API rejects, so the app retries forever.
- With a local receiver, nothing is ever in range.
- With airplanes.live, the radius is quietly clamped to 1 nm.

**Fix:** validate once in `Settings::resolve`:
- latitude must be in −90..=90
- longitude in −180..=180
- radius above 0 and at most 463 km (the 250 nm API limit)
- minimum elevation in −5..=89

Give a one-line error that says where the bad value came from (flag or config file).

**Done when:** each bad value fails with a clear message, with tests.

### 5. Rate limits aren't honoured

When OpenSky answers 429 the provider reads `X-Rate-Limit-Retry-After-Seconds`, puts it in an error string, and the fetcher then retries on its normal backoff (at most 2 minutes). Once the daily quota is gone the wait can be many hours, so overflight keeps knocking every 2 minutes all day. airplanes.live and local receivers can also answer 429 or 503 with a standard `Retry-After` header, which is ignored.

**Fix:**
- Return a typed error (for example `FetchError::RateLimited { retry_after: Duration }`) from providers.
- Make the fetcher sleep exactly that long.
- Show it in the status line in the user's terms: "OpenSky quota used up · next try 14:05".

**Done when:** a test provider that returns a rate-limit error is next called no earlier than the retry time.

### 6. Labels flicker when a field goes missing

`Track::apply` replaces callsign, registration and type with whatever the latest observation has, including `None`. OpenSky never sends registration or type and sometimes sends an empty callsign, so a label can flip between `DLH4AB` and `3c675a` from one update to the next.

**Fix:** only overwrite these fields when the new observation has a value. Keep the last known one otherwise.

**Done when:** a test that applies an observation without a callsign keeps the old label.

## Improvements

### 7. Show satellites only when you could actually see them

Satellites are drawn whenever they're above the horizon and the sky isn't in day mode. You can only see one with your eyes when it's in sunlight and your sky is dark, so today the ISS appears overhead while it's in Earth's shadow, where nobody can see it.

**Fix:**
- Compute whether the satellite is sunlit with a cylindrical Earth-shadow test using the sun vector you already compute in `sky.rs`.
- Draw visible satellites brightly and the rest faintly, or not at all.
- Propagate once per second instead of every frame.
- Make satellites selectable with `Tab`, with name, altitude and "visible now / in shadow" in the detail box.

### 8. Horizon mode: let people turn around

The side view always looks north, so anything to the south is invisible, and nothing tells you that.

**Fix:**
- `←`/`→` turn the view in 45° steps, and the top-left label updates ("looking south-east").
- Show a count of aircraft out of view at each edge ("‹ 3", "2 ›").
- `horizon_arrow` only looks at the east–west part of a plane's motion, so a plane flying straight towards or away from you is drawn as `→`. Use a distinct glyph such as `•` when most of its motion is along your line of sight.

### 9. Overlapping labels

In busy airspace, callsign labels overwrite each other. Before drawing a label, check whether the cells are already taken. If they are, try the other side of the arrow, then one row up or down, and drop the label if nothing fits. The selected aircraft always gets its label.

### 10. Fix the "rare type" list

The list mixes real ICAO type designators with ones that won't appear in the data. `AN124` isn't an ICAO code (the An-124 is `A124`, which is already in the list), the An-225 (`A225`) was destroyed in 2022, and Concorde (`CONC`) retired in 2003.
- Replace them with types you might actually see, such as `A124`, `A388`, `B748`, `C5M`, `B52H`, `A400`, `V22`, `C17`, `BLCF` (Dreamlifter) and `A3ST` (Beluga).
- Let the config file add to the list (`rare_types = ["..."]`).

### 11. Draw the Moon in daytime

The Moon is often visible during the day, but bodies are only drawn when the sky isn't in day mode. Draw the Moon whenever it's above the horizon, and keep the planets for twilight and night.

## New features

These are optional and in rough order of payoff for effort.

### 12. "Coming overhead" prediction

You already dead-reckon every aircraft. For each one, work out its highest point in your sky over the next 5 minutes and when it gets there. Show the best one in the status line: "DLH4AB will pass 72° up in 1:40". For a "what's that plane?" tool, this is the feature that makes people look up at the right moment.

### 13. Real stars

Replace the random star field with the few hundred brightest stars from the Yale Bright Star Catalogue (public domain), converted with the same `horizontal()` you use for planets. The night sky then shows real constellations that turn through the night, and people can actually find Orion. Constellation lines are an optional toggle (`c`).

### 14. Spotter's logbook

`--log <file.csv>` appends one row per aircraft when it leaves the sky: time, callsign, registration, type, and the highest elevation it reached. It's a small feature, but plane spotters keep logbooks like this.

### 15. Route lookup for the selected aircraft

When an aircraft is selected, look up its route (origin → destination) from a free callsign database such as adsbdb, cache the result per callsign for a day, and show it in the detail box. Check the service's terms and rate limits first, keep it off for `--demo`, and fail silently.

### 16. ISS pass alerts

With the TLE cache from item 2 in place, predict the next visible ISS pass for the observer: start time, highest elevation, direction. Show it in the status line during twilight, and show a notification when a pass starts.

### 17. Click to select

Enable mouse capture and let a click select the nearest aircraft or satellite within a few cells. Add `--no-mouse` for people who want text selection.

### 18. Opt-in bell

`--bell` rings the terminal bell when an unusual aircraft appears (emergency squawks, military, rare types). Off by default, and never in `--demo`.

---

Celestrak usage policy referenced in item 2: https://celestrak.org/usage-policy.php
