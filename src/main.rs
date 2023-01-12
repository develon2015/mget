use reqwest::{Client, header, header::{ACCEPT_RANGES, CONTENT_LENGTH}};
use crossterm::{cursor, queue, terminal};
use tokio::{sync::Mutex, fs::OpenOptions};
use std::sync::Arc;

#[derive(Debug)]
struct Properties {
    t: u64,
    url: String,
    o: Option<String>,
    proxy: Option<String>,
}

static mut CONFIG: Properties = Properties{ t: 3, url: String::new(), o: None, proxy: None };
static UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/108.0.0.0 Safari/537.36";

unsafe fn parse_args() {
    let mut args = std::env::args().skip(1);
    while let Some(arg)= args.next() {
        match arg.as_str() {
            "--thread" | "-t" => {
                let t = args.next().unwrap();
                let t = t.parse::<u64>().unwrap();
                CONFIG.t = t;
            },
            "--output" | "-o" => {
                let output = args.next().unwrap();
                CONFIG.o = Some(output);
            },
            "--proxy" | "-p" => {
                let proxy = args.next().unwrap();
                CONFIG.proxy = Some(proxy);
            }
            _ => {
                CONFIG.url = arg;
            },
        }
    }
    #[cfg(debug_assertions)]
    println!("{:#?}", CONFIG);
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    unsafe { parse_args() };
    let url = unsafe { &CONFIG.url };
    mget(url).await?;
    Ok(())
}

async fn mget(url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let url = url.clone();
    let client = create_client(url);
    println!("query resource...");
    let resp = client
        .head(url)
        .header(header::RANGE, "bytes=0-")
        .send()
        .await;
    let resp = match resp {
        Ok(resp) => resp,
        Err(_e) => {
            println!("HEAD request failed!");
            return Ok(());
        }
    };
    #[cfg(debug_assertions)]
    println!("{:#?}", resp);
    let headers = resp.headers();
    let accept_ranges = headers.get(ACCEPT_RANGES);
    let content_length = headers.get(CONTENT_LENGTH);
    if accept_ranges == None {
        println!("look like does not support Accept-Ranges!");
    }
    if content_length == None {
        println!("look like does not support Content-Length!");
        return Ok(());
    }
    let content_length: u64 = content_length.unwrap().to_str()?.parse()?;
    download(client, url, content_length).await?;
    Ok(())
}

fn create_client(url: &str) -> Client {
    // let client: Client = Client::new();
    let mut headers = header::HeaderMap::new();
    headers.insert(header::USER_AGENT, header::HeaderValue::from_static(UA));
    headers.insert(header::REFERER, header::HeaderValue::from_str(url).unwrap());
    if let Some(proxy) = unsafe { &CONFIG.proxy } {
        Client::builder()
            .proxy(reqwest::Proxy::http(proxy).unwrap())
    } else {
        Client::builder()
    }
        .default_headers(headers)
        .build()
        .unwrap()
}

async fn download(client: Client, url: &str, content_length: u64) -> Result<(), Box<dyn std::error::Error>> {
    use tokio::io::AsyncWriteExt;
    use tokio::fs::File;
    use tokio::io::AsyncSeekExt;

    // Create a new file and truncate it to 0 length if it already exists
    let o = unsafe { &CONFIG.o };
    let file_path;
    if let Some(o) = o {
        file_path = o.as_str();
    } else {
        file_path = "output.mget";
    }
    let file = File::create(file_path).await?;
    file.set_len(content_length).await?;
    drop(file);

    let t = unsafe { CONFIG.t };
    println!("content_length: {content_length}");
    let mut tasks = vec![];
    let count = Arc::new(Mutex::new(0));
    for n in 0..t {
        let client = client.clone();
        let count = count.clone();
        let mut file = OpenOptions::new().write(true).open(file_path).await?;
        let url = url.to_string();
        let task = tokio::spawn(async move {
            let sr = n * (content_length / t);
            let r = (n + 1) * (content_length / t) - 1;
            let range = if n + 1 < t { format!("bytes={sr}-{r}") } else { format!("bytes={sr}-") };
            let mut res = client.get(url).header(header::RANGE, range).send().await.unwrap();
            file.seek(std::io::SeekFrom::Start(sr)).await.unwrap();
            while let Some(chunk) = res.chunk().await.unwrap() {
                file.write_all(&chunk).await.unwrap();
                let mut count = count.lock().await;
                *count += chunk.len() as u64;
            }
        });
        tasks.push(task);
    }

    println!("starting download...");
    loop {
        let count = *count.lock().await;
        queue!(std::io::stdout(), cursor::MoveUp(1)).unwrap();
        queue!(std::io::stdout(), terminal::Clear(terminal::ClearType::FromCursorDown)).unwrap();
        if count >= content_length {
            println!("file {file_path} saved");
            break;
        }
        println!("Progress: {:.2}% {count}", (count as f64 / content_length as f64 ) * 100_f64);
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    for task in tasks {
        task.await?;
    }

    Ok(())
}
