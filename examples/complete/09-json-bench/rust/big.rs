// 09-json-bench (Rust): same workload as the Spectra twin.
// 20 manual JSON encode/decode roundtrips per round, 5 rounds.
// Checksums: count = 100, sum_ids = 950, sum_scores = 950.

fn parse(doc: &str) -> Option<(i64, String, i64)> {
    let id_key = "\"id\":";
    let id_start = doc.find(id_key)? + id_key.len();
    let id_rest = &doc[id_start..];
    let id_end = id_rest.find(',')?;
    let id: i64 = id_rest[..id_end].parse().ok()?;
    let label_key = "\"label\":\"";
    let label_start = doc.find(label_key)? + label_key.len();
    let label_rest = &doc[label_start..];
    let label_end = label_rest.find('"')?;
    let label = label_rest[..label_end].to_string();
    let score_key = "\"score\":";
    let score_start = doc.find(score_key)? + score_key.len();
    let score: i64 = doc[score_start..].trim_end_matches('}').parse().ok()?;
    Some((id, label, score))
}

fn main() {
    const PER_ROUND: i64 = 60;
    const ITERS: i64 = 10;
    let mut sum_ids: i64 = 0;
    let mut sum_scores: i64 = 0;
    let mut count: i64 = 0;
    for _ in 0..ITERS {
        for id in 0..PER_ROUND {
            let label = format!("u{}", id);
            let score = id % 100;
            let enc = format!("{{\"id\":{},\"label\":\"{}\",\"score\":{}}}", id, label, score);
            match parse(&enc) {
                Some((got_id, got_label, got_score))
                    if got_id == id && got_label == label && got_score == score =>
                {
                    sum_ids += got_id;
                    sum_scores += got_score;
                    count += 1;
                }
                _ => {
                    eprintln!("roundtrip mismatch at id {}", id);
                    std::process::exit(1);
                }
            }
        }
    }
    if count != 600 || sum_ids != 17700 || sum_scores != 17700 {
        eprintln!("checksum mismatch");
        std::process::exit(1);
    }
    println!("JSON ok objs=600 sum_ids=17700");
}
