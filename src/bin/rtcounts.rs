use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};

type Void = Result<(), Box<dyn std::error::Error>>;

fn main() -> Void {
    let mut rts = HashMap::new();
    let mut rt_counts = HashMap::new();
    let mut screen_name_forms = HashMap::new();
    let args = std::env::args().collect::<Vec<_>>();

    let entries = fs::read_dir(&args[1]).unwrap();

    for entry in entries {
        let reader = BufReader::new(fs::File::open(entry?.path())?);
        for res in reader.lines() {
            let line = res?;
            let fields: Vec<_> = line.split(',').collect();
            let rter = fields[0].to_string();
            let rted = fields[1].to_string();
            let rter_lc = rter.to_lowercase();
            let rted_lc = rted.to_lowercase();
            let rter_id = fields[2].parse::<u64>()?;
            let rted_id = fields[3].parse::<u64>()?;

            let rt_count = rt_counts.entry(rter_id).or_insert(0);
            *rt_count += 1;

            let rter_form_counts = screen_name_forms
                .entry(rter_lc.clone())
                .or_insert_with(|| HashMap::new());
            let rter_form_count = rter_form_counts.entry(rter).or_insert(0);
            *rter_form_count += 1;

            let rted_form_counts = screen_name_forms
                .entry(rted_lc.clone())
                .or_insert_with(|| HashMap::new());
            let rted_form_count = rted_form_counts.entry(rted).or_insert(0);
            *rted_form_count += 1;

            let for_rter = rts.entry(rter_lc).or_insert_with(|| HashMap::new());
            let pairs = for_rter.entry(rted_lc).or_insert_with(|| HashSet::new());

            pairs.insert((rter_id, rted_id));
        }
    }

    let screen_name_map = screen_name_forms
        .iter()
        .map(|(k, v)| (k, v.iter().max_by_key(|(_, count)| *count).unwrap().0))
        .collect::<HashMap<_, _>>();
    let mut screen_names = screen_name_map.iter().collect::<Vec<_>>();
    screen_names.sort_unstable_by_key(|(_, form)| form.to_string());

    let rt_counts_count = rt_counts.len();
    for (rt, count) in rt_counts.into_iter().filter(|(_, count)| *count > 1) {
        eprintln!("Multiple occurrences: {} for {}", count, rt);
    }
    eprintln!("{} total distinct retweets", rt_counts_count);

    for (screen_name, form) in screen_names {
        if let Some(for_rter) = rts.get(*screen_name) {
            let mut other = for_rter
                .iter()
                .map(|(k, v)| (screen_name_map.get(k).unwrap(), v.len()))
                .collect::<Vec<_>>();
            other.sort_unstable_by_key(|(rted, _)| rted.to_string());

            for (rted, count) in other {
                println!("{},{},{}", form, rted, count);
            }
        }
    }

    Ok(())
}
