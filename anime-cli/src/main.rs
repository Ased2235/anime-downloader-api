extern crate term_size;

mod anime_dl;
mod anime_find;

use getopts::Options;
use std::path::{Path};
use std::ffi::OsStr;
use std::process::exit;
use std::sync::mpsc::{channel, Sender, Receiver};
use std::thread;

use pbr::{MultiBar, Pipe, ProgressBar, Units};
use std::error::Error;

static IRC_SERVER: &str = "irc.rizon.net:6667";
static IRC_CHANNEL: &str = "nibl";
static IRC_NICKNAME: &str = "randomRustacean";


const AUDIO_EXTENSIONS: &'static [&'static str] = &["aif", "cda", "mid", "midi", "mp3",
                                                    "mpa", "ogg", "wav", "wma", "wpl"];

const VIDEO_EXTENSIONS: &'static [&'static str] = &["3g2", "3gp", "avi", "flv", "h264",
                                                    "m4v", "mkv", "mov", "mp4", "mpg",
                                                    "mpeg", "rm", "swf", "vob", "wmv"];


fn main() {
    let args: Vec<String> = std::env::args().collect(); // We collect args here
    let program = args[0].clone();
    let mut opts = Options::new();
    opts.optopt("q", "query", "Query to run", "QUERY")
        .optopt("e", "episode", "Episode number", "NUMBER")
        .optopt("b", "batch", "Batch end number", "NUMBER")
        .optopt("r", "resolution", "Resolution", "NUMBER")
        .optflag("h", "help", "print this help menu");
   
    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(error) => {
            eprintln!("{}.", error);
            eprintln!("{}", opts.short_usage(&program));
            exit(1);
        }
    };

   // let cli = true  // Are we in cli mode or prompt mode?

    let mut query: String;
    let resolution: Option<u16>;
    let episode: Option<u16>;
    let mut batch: Option<u16>;

        resolution = match matches.opt_str("r").as_ref().map(String::as_str) {
            Some("0") => None,
            Some(r) => Some(parse_number(String::from(r))),
            None => Some(720),
        };

        query = matches.opt_str("q").unwrap();

        episode = match matches.opt_str("e") {
            Some(ep) => Some(parse_number(ep)),
            None => None
        };

        batch = match matches.opt_str("b") {
            Some(b) => Some(parse_number(b)),
            None => None
        };

    query = query + match resolution { // If resolution entered, add a resolution to the query
        Some(x) => format!(" {}", x),
        None => "".to_string(),
    }.as_str();

    if batch.is_some() && batch.unwrap() < episode.unwrap_or(1) { // Make sure batch end is never smaller than episode start
        batch = episode;
    }

    let mut dccpackages = vec![];

    let mut num_episodes = 0;  // Search for packs, verify it is media, and add to a list
    for i in episode.unwrap_or(1)..batch.unwrap_or(episode.unwrap_or(1)) + 1 {
        match anime_find::find_package(&query, &episode.or(batch).and(Some(i))) {
            Ok(p) => {
                match Path::new(&p.filename).extension().and_then(OsStr::to_str) {
                    Some(ext) => {
                        if !AUDIO_EXTENSIONS.contains(&ext) && !VIDEO_EXTENSIONS.contains(&ext) {
                            eprintln!("Warning, this is not a media file! Skipping");
                        } else {
                            println!("{}", &p.filename);
                            dccpackages.push(p);
                            num_episodes += 1;
                        }
                    },
                    _ => { eprintln!("Warning, this file has no extension, skipping"); }
                }
            },
            Err(e) => {
                eprintln!("{}", e);
            }
        };
    }

    if num_episodes == 0 { exit(1); }
    
    // unless your username is shelltear, change it to correct one before compiling. otherwise this wont work
    let dir_path = Path::new("/home/shelltear/AnimeDownloads").to_owned();

    let terminal_dimensions  = term_size::dimensions();

    let mut channel_senders  = vec![];
    let mut multi_bar = MultiBar::new();
    let mut multi_bar_handles = vec![];
    let (status_bar_sender, status_bar_receiver) = channel();

    let mut safe_to_spawn_bar = true; // Even if one bar is safe to spawn, sending stdout outputs will interfere with the bars
    for i in 0..dccpackages.len() { //create bars for all our downloads
        let (sender, receiver) = channel();
        let handle;

        let pb_message;
        match terminal_dimensions {
            Some((w, _)) => {
                let acceptable_length = w / 2;
                if &dccpackages[i].filename.len() > &acceptable_length { // trim the filename
                    let first_half = &dccpackages[i].filename[..dccpackages[i].filename.char_indices().nth(acceptable_length / 2).unwrap().0];
                    let second_half = &dccpackages[i].filename[dccpackages[i].filename.char_indices().nth_back(acceptable_length / 2).unwrap().0..];
                    if acceptable_length > 50 { // 50 and 35 are arbitrary numbers
                        pb_message = format!("{}...{}: ", first_half, second_half);
                    } else if acceptable_length > 35 {
                        pb_message = format!("...{}: ", second_half);
                    } else {
                        pb_message = format!("{} added to list", dccpackages[i].filename);
                        safe_to_spawn_bar = false;
                    }
                } else {
                    pb_message = format!("{}: ", dccpackages[i].filename);
                }
            },
            None => {
                pb_message = format!("{} added to list", dccpackages[i].filename);
                safe_to_spawn_bar = false;
            },
        };
        let progress_bar;
        if safe_to_spawn_bar {
            let mut pb = multi_bar.create_bar(dccpackages[i].sizekbits as u64);
            pb.set_units(Units::Bytes);
            pb.message(&pb_message);
            progress_bar = Some(pb);
        } else { // If we can't spawn a bar, we just issue normal stdout updates
            progress_bar = None;
            multi_bar.println(&pb_message);
        }

        let status_bar_sender_clone = status_bar_sender.clone();
        handle = thread::spawn(move || { // create an individual thread for each bar in the multibar with its own i/o
            update_bar(progress_bar, receiver, status_bar_sender_clone);
        });

        channel_senders.push(sender);
        multi_bar_handles.push(handle);
    }

    let mut status_bar = None;
    if safe_to_spawn_bar {
        let mut sb = multi_bar.create_bar(dccpackages.len() as u64);
        sb.set_units(Units::Default);
        sb.message(&format!("{}: ", "Waiting..."));
        status_bar = Some(sb);
    }

    let status_bar_handle = thread::spawn(move || {
        update_status_bar(status_bar, status_bar_receiver);
    });
    multi_bar_handles.push(status_bar_handle);

    let _ = thread::spawn(move || { // multi bar listen is blocking
        multi_bar.listen();
    });

    let irc_request = anime_dl::IRCRequest {
        server: IRC_SERVER.to_string(),
        channel: IRC_CHANNEL.to_string(),
        nickname: IRC_NICKNAME.to_string(),
        bot: dccpackages.clone().into_iter().map(|package| package.bot).collect(),
        packages: dccpackages.clone().into_iter().map(|package| package.number.to_string()).collect(),
    };

    match anime_dl::connect_and_download(irc_request, channel_senders, status_bar_sender, dir_path.clone()) {
        Ok(_) => {},
        Err(e) => {
            eprintln!("{}", e);
            exit(1);
        }
    };
   
    multi_bar_handles.into_iter().for_each(|handle| handle.join().unwrap());
}

fn update_status_bar(progress_bar: Option<ProgressBar<Pipe>>, receiver: Receiver<String>) {
    let reader = |num: Result<String, _>, msg: &str| { match num {
        Ok(p) => p,
        Err(_) => {
            eprintln!("{}", msg);
            exit(1);
        },
    }};

    if progress_bar.is_some() {
        let mut pb = progress_bar.unwrap();
        pb.tick();
        let mut progress = reader(receiver.recv(), "Error updating status bar");

        while !progress.eq("Success") {
            pb.tick();
            if progress.eq("Episode Finished Downloading") {
                pb.inc();
            }

            pb.message(&format!("{} ", progress));
            progress = reader(receiver.recv(), "Error updating status bar");
        }
        pb.message(&format!("{} ", progress));
        pb.tick();
        pb.finish();
    } else {
        let mut progress = reader(receiver.recv(), "Error updating status");

        while !progress.eq("Success") {
            progress = reader(receiver.recv(), "Error updating status");
        }
    }
}

fn update_bar(progress_bar: Option<ProgressBar<Pipe>>, receiver: Receiver<i64>, _status_bar_sender: Sender<String>) {
    let reader = |num: Result<i64, _>, msg: &str| { match num {
        Ok(p) => p,
        Err(_) => {
            eprintln!("{}", msg);
            exit(1);
        },
    }};

    if progress_bar.is_some() {
        let mut pb = progress_bar.unwrap();
        pb.tick();

        let mut progress = reader(receiver.recv(), "Error updating progress bar");

        while progress > 0 {
            pb.set(progress as u64);

            progress = reader(receiver.recv(), "Error updating progress bar");
        }
        pb.finish();
    } else {
        let mut progress = reader(receiver.recv(), "Error updating progress");

        while progress > 0 {
            progress = reader(receiver.recv(), "Error updating progress");
        }
    }
}

fn parse_number(str_num: String) -> u16 {
    let c_str_num = str_num.replace(|c: char| !c.is_numeric(), "");
    match c_str_num.parse::<u16>() {
        Ok(e) => e,
        Err(err) => {
            if err.description().eq_ignore_ascii_case("cannot parse integer from empty string") { return 0 }
            eprintln!("Input must be numeric.");
            exit(1);
        }
    }
}
