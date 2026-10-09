complete -c overflight -l lat -d 'Observer latitude in decimal degrees' -r
complete -c overflight -l lon -d 'Observer longitude in decimal degrees' -r
complete -c overflight -l alt -d 'Observer height above sea level, in metres' -r
complete -c overflight -l source -d 'Where the aircraft data comes from [default: adsb-lol]' -r -f -a "adsb-lol\t''
airplanes-live\t''
local\t''
opensky\t''"
complete -c overflight -l url -d 'URL of a local receiver\'s aircraft.json (with --source local)' -r
complete -c overflight -l radius-km -d 'How far out to look, in kilometres [default: 80]' -r
complete -c overflight -l min-elevation -d 'Ignore aircraft below this elevation, in degrees [default: 0]' -r
complete -c overflight -l interval -d 'Seconds between updates, no faster than the source allows' -r
complete -c overflight -l log -d 'Append a CSV row per aircraft when it leaves the sky or you quit' -r -F
complete -c overflight -l colors -d 'Colour support: auto, truecolor, 256 or none [default: auto]' -r -f -a "auto\t'24-bit if `COLORTERM` says so, else 256 colours; no colour under `NO_COLOR`'
truecolor\t''
256\t''
none\t''"
complete -c overflight -l record -d 'Directory to save raw responses into, for building fixtures' -r -F
complete -c overflight -l time -d 'Fake the current time as an RFC 3339 timestamp, for checking palettes' -r
complete -c overflight -l tle-group -d 'Celestrak group to draw satellites from' -r
complete -c overflight -l demo -d 'Replay recorded traffic from the bundled fixture; no network needed'
complete -c overflight -l screensaver -d 'Screensaver mode: any key exits'
complete -c overflight -l no-mouse -d 'Do not capture the mouse, so you can select text as usual'
complete -c overflight -l no-routes -d 'Do not look up the selected aircraft\'s route (no adsbdb request)'
complete -c overflight -l bell -d 'Ring the terminal bell when an unusual aircraft appears'
complete -c overflight -l dump -d 'Fetch once, print a table of aircraft, then exit'
complete -c overflight -s h -l help -d 'Print help (see more with \'--help\')'
complete -c overflight -s V -l version -d 'Print version'
