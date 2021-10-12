use std::path::Path;

type Void = Result<(), Box<dyn std::error::Error>>;

#[tokio::main]
async fn main() -> Void {
    let args: Vec<String> = std::env::args().collect();
    let store_path = args.get(1).unwrap();
    let digests_path = args.get(2).unwrap();

    use std::io::{self, BufRead};
    let file = std::fs::File::open(digests_path)?;
    let digests: Vec<String> = io::BufReader::new(file).lines().collect::<Result<_, _>>()?;

    use futures::stream::StreamExt;

    futures::stream::iter(digests)
        .map(|digest| {
            let store_path = store_path.clone();
            tokio::spawn(async move {
                let p1 = format!(
                    "{}/data/other/{}/{}.gz",
                    store_path,
                    digest.chars().next().unwrap(),
                    digest
                );
                let p2 = format!(
                    "{}/data/valid/{}/{}.gz",
                    store_path,
                    digest.chars().next().unwrap(),
                    digest
                );
                let mut path = Path::new(&p1);

                if !path.exists() {
                    path = Path::new(&p2);
                }
                let file = std::fs::File::open(path)?;

                let mut found = false;
                let mut output = "".to_string();

                for result in io::BufReader::new(flate2::read::GzDecoder::new(file)).lines() {
                    let line = result?;
                    if line.contains("rel=\"canonical\"") || line.contains("rel='canonical'") {
                        let i = line.find("href=").unwrap();
                        let link = line
                            .chars()
                            .skip(i + 6)
                            .take_while(|c| *c != '"')
                            .collect::<String>();
                        output = format!("{},{}", digest, link);
                        found = true;
                        break;
                    }
                }

                if !found {
                    output = format!("{},", digest);
                }
                let res: std::io::Result<String> = Ok(output);
                res
            })
        })
        .buffer_unordered(32)
        .for_each(|result| async {
            let output = result.unwrap().unwrap();

            println!("{}", output);
        })
        .await;

    Ok(())
}
