# tally

tally is a mini-service designed for accepting syslog UDP packets from bunny.net, to give you basic statistics when putting their CDN in front of your site. Set tally up, configure your zone in the private web admin, and then go to the pull zone in bunny.net, Security -> Logging, and input the domain and UDP port tally can be reached at, setting the log format to plain text, and it will start aggregating metrics from the logs. If you configure a token you need to set that too.

Since it does not store the complete logs, tally has minimal runtime requirements, as of writing this my instance is using 20MB of memory and basically no CPU. Data is stored in a Fjall DB, in hourly rollups. Additionally it tracks estimated counts of the top-k of referring sites, user agents, and paths, using the Space-Saving algorithm. You can also choose specific query params to track the same way.

Each pull zone can be configured in the private web admin with basic settings and a token. Syslog lines from unknown pull zones or with a mismatching token are dropped.

## stats.json

For pull zones configured as public, tally will serve `/<zone>/stats.json?range=day|week|month|year|all`. Here's an example response from [typeahead.cove.town](https://typeahead.cove.town).

```json
{
   "zone": "typeahead",
   "period": "hour",
   "series": [
     [1794060000, 351],
     ...
   ],
   "requests": 4018,
   "statuses": { "2xx": 3919, "4xx": 27, "5xx": 72 },
   "cache": { "-": 28, "HIT": 641, "MISS": 301, "STALE": 3048 },
   "countries": { "BR": 505, "DE": 521, "FI": 727, "GB": 198, "NL": 519, "SG": 514, "US": 1034 },
   "clients": [
     ["Larm", 3581],
     ["Safari", 148],
     ["Firefox", 147],
     ["req", 85],
     ["Chrome", 29],
     ["curl", 28]
   ],
   "referrers": [
     ["none", 3697],
     ["typeahead.cove.town", 250],
     ["johanna.wisp.place", 35],
     ["127.0.0.1:8765", 27],
     ["sites.wisp.place", 9]
   ],
   "paths": [
     ["/xrpc/app.bsky.actor.searchActorsTypeahead", 3659],
     ["/xrpc/town.cove.actor.searchActorsTypeahead", 359]
   ],
   "params": {
     "q": [
       ["nel", 42],
       ["jola.dev", 10],
       ["oyster.", 6]
     ]
   }
 }
```

## Configuration

| Variable | Default | |
|---|---|---|
| `TALLY_SYSLOG_BIND` | `0.0.0.0:5514` | where Bunny sends logs (UDP) |
| `TALLY_PUBLIC_BIND` | `0.0.0.0:3000` | public port, for stats.json |
| `TALLY_INTERNAL_BIND` | `0.0.0.0:5515` | port for admin, don't expose to the internet |
| `TALLY_DATA_DIR` | `data` | fjall db |

## Development

```sh
just dev
just lint
just test
```

## License

Copyright 2026 Johanna Larsson. Licensed under the [Apache License 2.0](LICENSE).
