extern crate test;

use data_encoding::BASE32;
use flate2::read::GzDecoder;
use sha1::{Digest, Sha1};
use std::io::BufWriter;
use std::io::Read;

pub fn compute_digest<R: Read>(input: &mut R) -> std::io::Result<String> {
    let sha1 = Sha1::new();

    let mut buffered = BufWriter::new(sha1);
    std::io::copy(input, &mut buffered)?;

    let result = buffered.into_inner()?.finalize();

    let mut output = String::new();
    BASE32.encode_append(&result, &mut output);

    Ok(output)
}

pub fn compute_digest_gz<R: Read>(input: &mut R) -> std::io::Result<String> {
    compute_digest(&mut GzDecoder::new(input))
}

#[cfg(test)]
mod tests {
    use super::*;
    use test::Bencher;

    #[bench]
    fn bench_compute_digest_gz(b: &mut Bencher) {
        let paths = vec![
            "examples/wayback/53SGIJNJMTP6S626CVRCHFTX3OEWXB3E.gz",
            "examples/wayback/store/data/2G3EOT7X6IEQZXKSM3OJJDW6RBCHB7YE.gz",
            "examples/wayback/store/data/3KQVYC56SMX4LL6QGQEZZGXMOVNZR2XX.gz",
            "examples/wayback/store/data/5DECQVIU7Y3F276SIBAKKCRGDMVXJYFV.gz",
            "examples/wayback/store/data/AJBB526CEZFOBT3FCQYLRMXQ2MSFHE3O.gz",
        ];

        b.iter(|| {
            for path in &paths {
                let mut f = std::io::BufReader::new(std::fs::File::open(path).unwrap());
                compute_digest_gz(&mut f).unwrap();
            }
        });
    }
}
