use reqwest::{Client, header, header::{ACCEPT_RANGES, CONTENT_LENGTH}};
use crossterm::{cursor, queue, terminal};
use tokio::{sync::Mutex, fs::OpenOptions, io::AsyncReadExt, io::AsyncWriteExt};
use std::sync::Arc;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
struct Properties {
    t: u64,
    url: String,
    o: Option<String>,
    proxy: Option<String>,
    status: Option<Vec<i64>>,
    cookies: Option<String>,
}

static mut CONFIG: Properties = Properties{ t: 3, url: String::new(), o: None, proxy: None, status: None, cookies: None };
static UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/108.0.0.0 Safari/537.36";

async unsafe fn parse_args() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut file: Option<String> = None;
    while let Some(arg)= args.next() {
        match arg.as_str() {
            "--continue" | "-c" => {
                let status = args.next().unwrap();
                file = Some(status);
            },
            "--cookie" | "--cookies" | "-coo" => {
                let cookies = args.next().unwrap();
                let mut file = OpenOptions::new().read(true).open(cookies).await?;
                let mut cookies = String::with_capacity(20480);
                file.read_to_string(&mut cookies).await?;
                let lines = cookies.split("\r\n").collect::<Vec<&str>>();
                let mut cookies = String::with_capacity(20480);
                for e in &lines {
                    if e.starts_with("#") || e.is_empty() {
                        continue;
                    }
                    let e = e.split(char::is_whitespace).collect::<Vec<&str>>();
                    if e.len() != 7 {
                        continue;
                    }
                    cookies.push_str(&format!("{}={}; ", e[5], e[6]));
                }
                if lines.len() > 0 {
                    cookies.pop();
                    cookies.pop();
                }
                CONFIG.cookies = Some(cookies);
            },
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
    if let Some(file) = file {
        let mut file = tokio::fs::OpenOptions::new().read(true).open(file).await?;
        let mut buf = String::with_capacity(1024);
        file.read_to_string(&mut buf).await?;
        let mut config = serde_json::from_str::<Properties>(&buf).unwrap();
        if CONFIG.proxy.is_some() {
            config.proxy = CONFIG.proxy.clone();
        }
        if CONFIG.url != "" {
            config.url = CONFIG.url.clone();
        }
        if CONFIG.cookies != None {
            config.cookies = CONFIG.cookies.clone();
        }
        CONFIG = config;
    }
    #[cfg(debug_assertions)]
    println!("{:#?}", CONFIG);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    unsafe { parse_args().await? };
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
        // .header(header::RANGE, "bytes=0-")
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
    if let Some(cookies) = unsafe { &CONFIG.cookies } {
        headers.insert(header::COOKIE, header::HeaderValue::from_str(cookies).unwrap());
    }
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

async fn write_to_file(file: &mut tokio::fs::File, buffer: &mut Vec<u8>, count: &Mutex<u64>, sr: &mut u64) {
    file.write_all(&buffer).await.unwrap();
    let mut count = count.lock().await;
    *count += buffer.len() as u64;
    *sr += buffer.len() as u64;
    unsafe { buffer.set_len(0); }
}

async fn download(client: Client, url: &str, content_length: u64) -> Result<(), Box<dyn std::error::Error>> {
    use tokio::fs::File;
    use tokio::io::AsyncSeekExt;

    // Create a new file and truncate it to 0 length if it already exists
    let o = unsafe { &CONFIG.o };
    let file_path;
    if let Some(o) = o {
        file_path = o.as_str();
    } else {
        file_path = "output";
    }
    if *unsafe { &CONFIG.status } == None {
        let file = File::create(file_path).await?;
        file.set_len(content_length).await?;
        drop(file);
    }

    let mut status_file_path = file_path.to_string();
    status_file_path.push_str(".mget");
    let status_file = OpenOptions::new().create(true).write(true).open(status_file_path).await?;
    // status_file.set_len(0).await?;

    let t = unsafe { CONFIG.t };
    let map = Arc::new(Mutex::new(vec![0_i64; t as usize]));
    println!("content_length: {content_length}");
    let mut tasks = vec![];
    let count = Arc::new(Mutex::new(0));
    for n in 0..t {
        let should_sr = n * (content_length / t);
        let mut sr = should_sr;
        if let Some(status) = unsafe { &CONFIG.status } {
            let mut count = count.lock().await;
            let mut map = map.lock().await;
            if status[n as usize] == -1 {
                *count += if n + 1 < t { content_length / t } else { content_length - content_length / t * n };
                map[n as usize] = -1;
                continue;
            }
            sr = status[n as usize] as u64;
            // println!("线程{n}从{sr}继续下载");
            map[n as usize] = sr as i64;
            *count += sr - should_sr;
        }
        let client = client.clone();
        let count = count.clone();
        let mut file = OpenOptions::new().write(true).open(file_path).await?;
        let url = url.to_string();
        let map = map.clone();
        let mut status_file = status_file.try_clone().await?;
        let task = tokio::spawn(async move {
            'outer: loop {
                let r = (n + 1) * (content_length / t) - 1;
                let range = if n + 1 < t { format!("bytes={sr}-{r}") } else { format!("bytes={sr}-") };
                // println!("thread {n} starting bytes={sr}-{r} \n");
                let mut res = client.get(&url).header(header::RANGE, &range).send().await
                    .expect(&format!("thread {n} download failed ({sr}-{r})\n"));
                file.seek(std::io::SeekFrom::Start(sr)).await.unwrap();
                let mut buffer = Vec::<u8>::with_capacity(256 * 1024);
                'inner: loop {
                    match res.chunk().await {
                        Ok(Some(chunk)) => {
                            if buffer.capacity() < buffer.len() + chunk.len() {
                                write_to_file(&mut file, &mut buffer, &count, &mut sr).await;
                                let mut map = map.lock().await;
                                map[n as usize] = sr as i64;
                                unsafe {
                                    CONFIG.status = Some(map.clone());
                                    let status = serde_json::json!(CONFIG);
                                    status_file.seek(std::io::SeekFrom::Start(0)).await.unwrap();
                                    status_file.write_all(format!("{:#}", status).as_bytes()).await.unwrap();
                                }
                            }
                            buffer.append(&mut chunk.to_vec());
                        },
                        Ok(None) => {
                            break 'inner;
                        },
                        Err(e) => {
                            println!("thread {n} error: {e} \n");
                            std::thread::sleep(std::time::Duration::from_millis(2000));
                            println!("thread {n} retry: bytes={sr}-{r} \n");
                            continue 'outer;
                        },
                    }
                }
                // 处理剩余未写入文件的buffer
                if buffer.len() > 0 {
                    write_to_file(&mut file, &mut buffer, &count, &mut sr).await;
                }
                let mut map = map.lock().await;
                map[n as usize] = -1;
                unsafe {
                    CONFIG.status = Some(map.clone());
                    let status = serde_json::json!(CONFIG);
                    status_file.set_len(0).await.unwrap();
                    status_file.seek(std::io::SeekFrom::Start(0)).await.unwrap();
                    status_file.write_all(format!("{:#}", status).as_bytes()).await.unwrap();
                }
                break;
            }
        });
        tasks.push(task);
    }

    println!("starting download...");
    let mut last_count = 0;
    loop {
        let count = *count.lock().await;
        queue!(std::io::stdout(), cursor::MoveUp(1)).unwrap();
        queue!(std::io::stdout(), terminal::Clear(terminal::ClearType::FromCursorDown)).unwrap();
        if count >= content_length {
            println!("file {file_path} saved");
            break;
        }
        let map = map.lock().await;
        println!("{:?} Progress:  {:.2}%  {:.2}MB/{:.2}MB  {:.2}MB/s",
            map,
            (count as f64 / content_length as f64 ) * 100_f64,
            count as f64 / 1024_f64 / 1024_f64,
            content_length as f64 / 1024_f64 / 1024_f64,
            (count as f64 - last_count as f64 ) / 1024_f64 / 1024_f64 * 2.0,
        );
        last_count = count;
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    for task in tasks {
        task.await?;
    }

    Ok(())
}
