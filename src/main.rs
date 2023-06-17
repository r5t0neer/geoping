extern crate tokio;

use std::collections::HashMap;
use std::env::current_dir;
use std::fs;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::{PathBuf};
use std::str::FromStr;
use std::time::Duration;
use serde_json::Value;
use surge_ping::{PingIdentifier, PingSequence};
use tokio::time::{Instant, timeout};
use crate::iplookup::IpInfoClientWrapper;

mod iplookup;

type CountryCode = String;// like DE, PL
type City = String;
type IP = String;// either v4 or v6
type Rtt = f64;// Round Trip Time / latency / ping time

const IPINFO_SECRET: &str = "...";
const IPINFO_TIMEOUT: u64 = 15000;// ms
const MAX_RTT: f64 = 500.0f64;// ms
const PING_COUNT: u16 = 10;// per IP

fn obtain_country_code_from_filepath(path: &PathBuf) -> CountryCode
{
    path.file_name().unwrap().to_string_lossy()
        .split(".")
        .next().unwrap()
        .to_uppercase()
}

fn gather_files_with_ext(dir: &PathBuf, extension: &str) -> Vec<PathBuf>
{
    let mut paths = vec![];

    for dir in fs::read_dir(dir)
    {
        for file in dir.flatten()
        {
            if let Ok(kind) = file.file_type()
            {
                // regular file
                if kind.is_file()
                {
                    let path = file.path();

                    if let Some(ext) = path.extension()
                    {
                        if ext == extension
                        {
                            paths.push(path);
                        }
                    }
                }
            }
        }
    }

    paths
}

fn collect_servers() -> HashMap<CountryCode, Vec<(City, IP)>>
{
    let mut countries = HashMap::new();

    let paths = gather_files_with_ext(&current_dir().unwrap(), "json");

    println!("JSON files should be placed at {}", current_dir().unwrap().to_string_lossy());
    println!("Loaded {} countries", paths.len());

    for path in paths
    {
        let mut file = OpenOptions::new()
            .read(true)
            .open(&path)
            .unwrap();

        let cc = obtain_country_code_from_filepath(&path);

        let mut content = String::new();
        file.read_to_string(&mut content)
            .unwrap();

        let json: Value = serde_json::from_str(content.as_str()).unwrap();
        let entries = json
                                  .as_array()
                                  .unwrap();

        let mut cities = vec![];

        for entry in entries
        {
            let ip = entry["ip"].as_str().unwrap().to_string();
            let city = entry["city"].as_str().unwrap().to_string();

            cities.push((city, ip));
        }

        countries.insert(cc, cities);
    }

    countries
}

async fn ping_servers(servers: HashMap<CountryCode, Vec<(City, IP)>>, servers_count: u64) -> HashMap<CountryCode, Vec<(City, IP, Rtt)>>
{
    use surge_ping::{Config, Client};

    let mut rtts = HashMap::new();

    let cfg = Config::default();
    let client = Client::new(&cfg).unwrap();

    let timeo = Duration::from_millis(MAX_RTT as u64);

    let mut count_now = 0;

    for (cc, cities) in servers
    {
        // not using .iter().enumerate()
        // so no extra .clone() on e.g. city.clone() while pushing result

        let mut id = 0;

        for (city, ip) in &cities
        {
            let mut parsed_ip: Option<IpAddr> = None;

            if let Ok(addr) = Ipv4Addr::from_str(ip.as_str()) { parsed_ip = Some(IpAddr::V4(addr)); }
            if let Ok(addr) = Ipv6Addr::from_str(ip.as_str()) { parsed_ip = Some(IpAddr::V6(addr)); }

            if parsed_ip.is_none() { continue; }

            let mut pinger = client.pinger(parsed_ip.unwrap(), PingIdentifier(id as u16)).await;

            let mut min_rtt = MAX_RTT;
            'ping_loop: for i in 0..PING_COUNT
            {
                match timeout(timeo, pinger.ping(PingSequence(i), &[])).await
                {
                    Ok(ping_result) => {
                        if let Ok((_, duration)) = ping_result {

                            let rtt = (duration.as_nanos() as f64) / 1_000_000.0f64;

                            if rtt < min_rtt { min_rtt = rtt; }
                        }
                    }
                    Err(_) => { /* timed out */ break 'ping_loop; }
                }
            }

            if min_rtt < MAX_RTT
            {
                if id % 10 == 0
                {
                    println!("{} {:.0}%, {:.2} ms", cc, (((id as f64) / (cities.len() as f64)) * 100.0f64).round(), min_rtt);
                }

                rtts.entry(cc.clone())
                    .or_insert(vec![])
                    .push((city.clone(), ip.clone(), min_rtt));
            }

            id += 1;
        }

        count_now += cities.len();
        println!("Total {:.0}%", 100.0f64 * (count_now as f64) / (servers_count as f64));
    }

    rtts
}

async fn fill_empty_locations(rtts: &mut HashMap<CountryCode, Vec<(City, IP, Rtt)>>, servers_count: u64, ipinfo_client: &mut IpInfoClientWrapper)
{
    println!("Filling empty locations...");

    let mut count_now = 0u64;
    let mut pct_prev = 0.0f64;

    for (_, cities) in rtts.iter_mut()
    {
        for (city, ip, _) in cities.iter_mut()
        {
            if city.trim().is_empty()
            {
                match ipinfo_client.query(ip.as_str()).await
                {
                    Ok(res) => {
                        *city = res.city;
                    }
                    Err(err) => { println!("Could not get city for IP {}: {}", ip, err); }
                }
            }

            count_now += 1;
            let pct = ((count_now as f64) / (servers_count as f64)) * 100.0f64;
            if pct >= (pct_prev + 5.0f64)
            {
                println!("{:.01}%", pct);

                pct_prev = pct;
            }
        }
    }

    println!("100%");
}

async fn fix_countries(rtts: &mut HashMap<CountryCode, Vec<(City, IP, Rtt)>>, servers_count: u64, ipinfo_client: &mut IpInfoClientWrapper)
{
    println!("Fixing locations...");

    let mut count_now = 0u64;
    let mut pct_prev = 0.0f64;

    let mut modifiers = vec![];

    for (cc, cities) in rtts.iter()
    {
        for (i, (city, ip, rtt)) in cities.iter().enumerate()
        {
            match ipinfo_client.query(ip.as_str()).await
            {
                Ok(details) => {
                    if details.country != *cc
                    {
                        modifiers.push((cc.clone(), i, details.country, city.clone(), ip.clone(), *rtt));
                    }
                }
                Err(err) => { println!("Could not resolve country for city {} for IP {}: {}", city, ip, err); }
            }

            count_now += 1;
            let pct = ((count_now as f64) / (servers_count as f64)) * 100.0f64;
            if pct >= (pct_prev + 5.0f64)
            {
                println!("{:.01}%", pct);

                pct_prev = pct;
            }
        }
    }

    for (cc_estimated, i, _, _, _, _) in modifiers.iter().rev()
    {
        rtts.get_mut(cc_estimated).unwrap().remove(*i);
    }

    for (_, _, cc_real, city, ip, rtt) in modifiers
    {
        if let Some(entry) = rtts.get_mut(&cc_real) {
            entry.push((city, ip, rtt));
        }
    }

    println!("100%");
}

fn generate_csv(rtts: &mut HashMap<CountryCode, Vec<(City, IP, Rtt)>>) -> String
{
    let mut csv = String::new();

    csv += "Country\tMin RTT\tMedian RTT\tAverage RTT\tMax RTT\n";

    let mut intermediate = vec![];

    for (cc, entries) in rtts
    {
        let mut min = MAX_RTT;
        let mut max = 0.0f64;
        let mut sum = 0.0f64;

        for (_, _, rtt) in entries.iter()
        {
            if *rtt < min { min = *rtt; }
            if *rtt > max { max = *rtt; }

            sum += *rtt;
        }

        let average = sum / (entries.len() as f64);

        let median = {
            let len = entries.len();

            entries.sort_by(|(_, _, rtt1), (_, _, rtt2)| rtt1.partial_cmp(rtt2).unwrap());

            if len % 2 == 0
            {
                (
                    entries[len / 2 - 1].2 +
                    entries[len / 2 - 1].2
                )
                / 2.0f64
            }
            else { entries[len / 2].2 }
        };

        intermediate.push((cc.clone(), min, median, average, max));
    }

    intermediate.sort_by(|(_, min1, _, _, _), (_, min2, _, _, _)| min1.partial_cmp(min2).unwrap());

    for (cc, min, median, average, max) in intermediate
    {
        csv += cc.as_str(); csv.push('\t');
        csv += format!("{:.03}", min).replace('.', ",").as_str(); csv.push('\t');
        csv += format!("{:.03}", median).replace('.', ",").as_str(); csv.push('\t');
        csv += format!("{:.03}", average).replace('.', ",").as_str(); csv.push('\t');
        csv += format!("{:.03}", max).replace('.', ",").as_str(); csv.push('\n');
    }

    csv
}

#[tokio::main]
async fn main()
{
    let mut timer = Instant::now();

    let mut ipinfo_client = IpInfoClientWrapper::new(
        IPINFO_SECRET,
        Duration::from_millis(IPINFO_TIMEOUT)
    ).unwrap();

    // 1. Ping

    println!("[Step 1] Pinging servers...");

    let servers = collect_servers();

    let mut count_total = 0u64;
    for (_, cities) in servers.iter()
    {
        count_total += cities.len() as u64;
    }

    let mut rtts = ping_servers(servers, count_total).await;

    // 2. Correct

    println!("[Step 2] Correcting locations...");

    //fill_empty_locations(&mut rtts, count_total, &mut ipinfo_client).await;
    fix_countries(&mut rtts, count_total, &mut ipinfo_client).await;

    // 3. Output

    println!("[Step 3] Generating CSV...");

    let csv = generate_csv(&mut rtts);

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open("rtt_result.csv")
        .unwrap();

    file.write_all(csv.as_bytes()).unwrap();
    file.flush().unwrap();

    println!("Done!, it took {}s", timer.elapsed().as_secs());
}
