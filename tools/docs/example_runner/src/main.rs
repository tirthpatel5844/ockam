use anyhow::Result;
use duct::cmd;
use rand::RngCore;
use ron::de::from_str;
use serde::Deserialize;
use std::fs::File;
use std::io::Read;
use std::net::SocketAddr;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::Duration;

pub type Step = String;

pub type Stage = Vec<Step>;

#[derive(Deserialize, Debug)]
pub struct Script {
    title: String,
    stages: Vec<Stage>,
}

fn run_stage(stage: Stage) -> Result<()> {
    let stop = Arc::new(AtomicBool::new(false));
    let finished = Arc::new(AtomicBool::new(false));

    let join_handles = Arc::new(Mutex::new(Vec::new()));

    let mut match_stack: Vec<String> = Vec::new();
    let mut out_order: Vec<String> = Vec::new();
    let mut outputs: Vec<String> = Vec::new();

    for mut step in stage {
        let stop = stop.clone();

        if stop.load(Ordering::Relaxed) {
            break;
        }

        let finished = finished.clone();
        let join_handles = join_handles.clone();

        if step.starts_with("sleep ") {
            let duration = step.split_off(6);
            let duration = duration.trim();
            let duration: u64 = duration.parse()?;
            println!("Sleeping for {} seconds", duration);
            sleep(Duration::from_secs(duration));
            continue;
        }

        if step.starts_with("match ") {
            let pattern = step.split_off(6);
            println!("Matching '{}' in output", pattern);
            loop {
                let mut f = File::open(outputs.last().unwrap())?;
                let mut s = String::new();
                f.read_to_string(&mut s)?;

                if let Some(index) = s.find(pattern.clone().as_str()) {
                    let mut matching = s.split_off(index);
                    if let Some(end) = matching.find('\n') {
                        matching.truncate(end);
                    }
                    println!("Matched '{}'", matching);
                    match_stack.push(matching.to_string());
                    break;
                }
                sleep(Duration::from_secs(1))
            }
            continue;
        }

        if step.starts_with("out ") {
            let index = step.split_off(4);
            let index: usize = index.parse()?;
            let matching = match_stack.get(index).unwrap();
            out_order.push(matching.clone());
            continue;
        }

        if step.starts_with("tcp ") {
            let step = step.split_off(4);
            let mut args = step.split(" ");
            let bytes = args.next().expect("missing tcp read length");
            let addr = args.next().expect("missing tcp host:port");
            let tcp_match = args.next().expect("expect byte match");

            let addr: SocketAddr = addr.parse()?;
            let mut tcp = std::net::TcpStream::connect(addr)?;
            let bytes: usize = bytes.parse().expect("invalid tcp read length");
            let mut buf: Vec<u8> = Vec::with_capacity(bytes);
            tcp.read(&mut buf)?;
            let buf = String::from_utf8(buf)?;
            if !buf.contains(tcp_match) {
                println!(
                    "No match for '{}' in TCP response from '{}'. Got '{}'",
                    tcp_match, addr, buf
                );
                break;
            }
            continue;
        }

        if step.starts_with("quit") {
            break;
        }

        let step = step.clone();
        let step_and_args = step.split_whitespace();
        let mut args = Vec::from(["run".to_string(), "--example".to_string()]);

        for arg in step_and_args {
            args.push(arg.to_string())
        }

        let output_file = format!("/tmp/exrun-{}", rand::thread_rng().next_u32());
        outputs.push(output_file.clone());

        let mut stdin = String::new();
        for out in &out_order {
            stdin += &*out;
            stdin += "\n";
        }

        println!("Out: {:#?}", output_file);

        let join_handle = std::thread::spawn(move || {
            let handle = cmd("cargo", args)
                .stdout_file(File::create(output_file.clone()).unwrap())
                .stdin_bytes(stdin)
                .start()
                .unwrap();
            while !stop.load(Relaxed) {
                match handle.try_wait() {
                    Ok(maybe_output) => match maybe_output {
                        Some(output) => {
                            println!(
                                "Output: {}",
                                String::from_utf8(output.clone().stdout).unwrap()
                            );
                            finished.store(true, Relaxed);
                            break;
                        }
                        _ => (),
                    },
                    Err(_) => {
                        std::process::exit(1);
                    }
                }
                sleep(Duration::from_secs(1));
            }
            handle.kill().unwrap();
        });
        join_handles.lock().unwrap().push(join_handle);
        sleep(Duration::from_secs(2));
    }
    stop.store(true, Relaxed);

    while !finished.load(Relaxed) {
        sleep(Duration::from_secs(1));
    }

    stop.store(true, Relaxed);
    let join_handles = join_handles.clone();
    let mut join_handles = join_handles.lock().unwrap();
    while !join_handles.is_empty() {
        let h = join_handles.pop().unwrap();
        h.join().unwrap();
    }
    Ok(())
}

fn run(script: Script) -> Result<()> {
    println!("Running {}", script.title);
    for stage in script.stages {
        run_stage(stage)?;
    }
    Ok(())
}

fn main() -> Result<()> {
    let file = std::env::args()
        .skip(1)
        .next()
        .expect("missing script file");
    let mut file = File::open(file).expect("unable to open script");
    let mut guide = String::new();

    file.read_to_string(&mut guide)?;

    let script: Script = from_str(guide.as_str()).expect("script is invalid");
    run(script)?;
    Ok(())
}
