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
