//! Contexte : les régressions réseau doivent être reproductibles même sans Internet.
//! Usage : server joue des réponses HTTP locales ; directory isole chaque scénario.
//! Aucun test automatique ne contacte IGN et aucune suppression ne sort de target/.

use std::{
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

pub fn directory() -> PathBuf {
    // L'horloge Windows peut donner la même date à deux tests simultanés.
    // Le compteur atomique attribue un numéro distinct, même entre threads.
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let number = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!(
            "test-regression-{}-{unique}-{number}",
            std::process::id()
        ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

pub fn cleanup(path: &Path) {
    let actual = std::fs::canonicalize(path).unwrap();
    let allowed =
        std::fs::canonicalize(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target")).unwrap();
    assert!(actual.starts_with(allowed));
    assert!(
        actual
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("test-regression-")
    );
    std::fs::remove_dir_all(actual).unwrap();
}

/// Renvoie l'URL locale et un fil d'exécution qui rendra toutes les requêtes reçues.
/// Les délais bornés transforment une requête oubliée en échec, sans bloquer les tests.
pub fn server(
    replies: Vec<(u16, &'static str, Vec<u8>)>,
) -> (String, thread::JoinHandle<Vec<reqwest::Url>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let mut requests = Vec::new();
        for (status, content_type, body) in replies {
            let deadline = Instant::now() + Duration::from_secs(8);
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "requête HTTP attendue absente");
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("serveur local : {error}"),
                }
            };
            // Sous Windows le socket accepté peut hériter du mode non bloquant.
            // Revenir explicitement au mode bloquant avant de lire les en-têtes.
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                assert_ne!(socket.read(&mut byte).unwrap(), 0);
                request.push(byte[0]);
                assert!(request.len() < 65536);
            }
            let text = String::from_utf8(request).unwrap();
            let target = text.split_whitespace().nth(1).unwrap();
            requests.push(reqwest::Url::parse(&format!("http://{address}{target}")).unwrap());
            write!(socket, "HTTP/1.1 {status} Test\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
            let _ = socket.write_all(&body);
        }
        requests
    });
    (format!("http://{address}/wfs"), handle)
}

pub fn client() -> reqwest::Client {
    // Ignorer le proxy de l'entreprise pour les connexions à 127.0.0.1.
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
}
