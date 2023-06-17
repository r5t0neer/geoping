# GeoPing

Takes in JSON files from public-dns.info, pings every listed server with ICMP and outputs statistics - min, average, median and max for each country.
Verifies listed countries from input CSV files with ipinfo.io and corrects dns servers by moving to proper country.

## Prerequisites

Account on IPInfo.io with at least 30k free requests if you wanna ping entire Europe.

Note: it takes 6 hours to ping every DNS server (10 times - to get more reliable latencies) in Europe
      if max RTT is set to 500 ms and you are in center of Europe,
      and probably 50 minutes if pinging 1 time each server

---

### Further improvements

- speeding up with prepared dns servers - few servers for each statistic metric (min, median, average, max; least servers for marginal cases and more as it hits slower servers in mean of latency dependent on network infrastructure - tier 1 interconnections, fiber network, ethernet network and slower ones) instead of pinging every server, so it does not take hours but minutes
- generate second CSV which includes large cities from each country to get more informative result
