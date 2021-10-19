use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};

type Void = Result<(), Box<dyn std::error::Error>>;

/// Iterate over all files indicating tweet availability in a directory and output the most recent
/// availability status for each tweet. Expected format is a two-column CSV file with tweet ID and
/// either a 0 or 1 indicating availability (where 0 means unavailable).
fn main() -> Void {
    let args = std::env::args().collect::<Vec<_>>();
    let mut entries = fs::read_dir(&args[1])
        .unwrap()
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());

    let mut availability = HashMap::new();

    for entry in entries {
        let reader = BufReader::new(fs::File::open(entry.path())?);
        for res in reader.lines() {
            let line = res?;
            let mut fields = line.split(',').map(|field| field.trim());

            let tweet_id = fields
                .next()
                .expect("Invalid format: no tweet ID")
                .parse::<u64>()
                .expect("Invalid format: bad tweet ID");
            let availability_code = fields
                .next()
                .expect("Invalid format: no availability")
                .parse::<usize>()
                .expect("Invalid format: bad availability");

            if availability_code != 0 && availability_code != 1 {
                panic!("Invalid format: bad availability");
            }

            availability.insert(tweet_id, availability_code == 1);
        }
    }

    let mut pairs = availability.into_iter().collect::<Vec<_>>();
    pairs.sort_unstable();

    for (tweet_id, is_available) in pairs {
        println!("{},{}", tweet_id, if is_available { "1" } else { "0" });
    }

    Ok(())
}
