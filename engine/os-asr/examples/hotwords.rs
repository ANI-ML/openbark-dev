//! Spike: does hotword biasing work with Parakeet (NeMo transducer) in sherpa-onnx?
//! cargo run -p os-asr --example hotwords --release -- <model dir> <wav> <start> <end> <hotwords...>
use os_asr::sherpa_onnx::*;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (dir, wav, start, end) = (&a[1], &a[2], a[3].parse::<f32>().unwrap(), a[4].parse::<f32>().unwrap());
    let hot: Vec<String> = a[5..].to_vec();
    // sherpa encodes hotwords with a BPE vocab ("token\tscore"); derive one from tokens.txt.
    let vocab = std::env::temp_dir().join("parakeet-bpe.vocab");
    let tokens = std::fs::read_to_string(format!("{dir}/tokens.txt")).unwrap();
    let v: String =
        tokens.lines().filter_map(|l| l.rsplit_once(' ').map(|(t, _)| format!("{t}\t0\n"))).collect();
    std::fs::write(&vocab, v).unwrap();
    let audio = os_asr::Audio::load_wav(std::path::Path::new(wav)).unwrap();
    let clip = audio.slice(start, end);
    for (method, use_hot) in
        [("greedy_search", false), ("modified_beam_search", false), ("modified_beam_search", true)]
    {
        let mut mc =
            OfflineModelConfig { num_threads: 4, provider: Some("cpu".into()), ..Default::default() };
        mc.transducer = OfflineTransducerModelConfig {
            encoder: Some(format!("{dir}/encoder.int8.onnx")),
            decoder: Some(format!("{dir}/decoder.int8.onnx")),
            joiner: Some(format!("{dir}/joiner.int8.onnx")),
        };
        mc.tokens = Some(format!("{dir}/tokens.txt"));
        mc.model_type = Some("nemo_transducer".into());
        if use_hot {
            mc.modeling_unit = Some("bpe".into());
            mc.bpe_vocab = Some(vocab.to_string_lossy().into_owned());
        }
        let cfg = OfflineRecognizerConfig {
            model_config: mc,
            decoding_method: Some(method.into()),
            max_active_paths: 4,
            hotwords_score: if use_hot { 2.0 } else { 0.0 },
            ..Default::default()
        };
        let Some(rec) = OfflineRecognizer::create(&cfg) else {
            println!("{method} hot={use_hot}: create failed");
            continue;
        };
        let stream =
            if use_hot { rec.create_stream_with_hotwords(&hot.join("\n")) } else { rec.create_stream() };
        stream.accept_waveform(16000, clip);
        rec.decode(&stream);
        println!("{method} hot={use_hot}: {}", stream.get_result().map(|r| r.text).unwrap_or_default());
    }
}
