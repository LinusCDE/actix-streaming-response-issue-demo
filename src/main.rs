use actix_web::body::SizedStream;
use actix_web::{get, middleware, web, App, HttpResponse, HttpServer, Responder, Result};
use log::{error, info};
use tokio::io::BufReader;
use tokio_util::io::ReaderStream;

mod testcase;

fn main() {
    // Init logging
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "DEBUG");
    }
    env_logger::builder().format_timestamp_millis().init();

    // Init actix rt and start webserver
    if let Err(fatal_error) = actix_rt::System::new().block_on(program()) {
        error!("Fatal ERR: {:?}", fatal_error);
        std::process::exit(1);
    }
}

async fn program() -> Result<()> {
    info!("Starting webserver...");
    run_server().await?;
    info!("Received signal to terminate. Bye...");
    Ok(())
}

pub async fn run_server() -> std::io::Result<()> {
    let server = HttpServer::new(|| {
        let cors = actix_cors::Cors::default().allow_any_origin(); // Changable for dev env

        App::new()
            // <snip>
            .wrap(middleware::Logger::default())
            //.wrap(middleware::Compress::default()) // Disabled for now (hurts usb image download speeds) | Feature: "compress-gzip"
            .wrap(middleware::NormalizePath::new(
                middleware::TrailingSlash::Trim,
            ))
            .wrap(middleware::DefaultHeaders::new().add(("Server", "HILmate")))
            .wrap(cors)
            .configure(add_routes)
        //.service(Files::new("/apidoc", OPTS.apidoc_folder.to_owned()).index_file("index.html"))
        // <snip>
        //.default_service(web::get().to(angular::handler))
    })
    .shutdown_timeout(1)
    .workers(4) // Reducing likelyhood/impact of issue #xxx (TODO: reduce to 1/2 when fixed)
    .bind(format!("{}:{}", "::", 8080))?;
    server.run().await?;
    Ok(())
}

#[rustfmt::skip]
pub fn add_routes(conf: &mut web::ServiceConfig) {
    conf.service(get_index)
        .service(get_testcase_data);
}

#[get("/")]
async fn get_index() -> Result<impl Responder> {
    Ok(concat!(
        "Hi, I'm a test page to check whether the server is still responding.\n\n",
        "How to test:\n",
        " - Go to /download\n",
        " - Have 4 downloads running ( = worker count)\n",
        " - Try to load this page again\n",
        "\n",
        "Downloading might already fail when less than 4 are already running.\n",
        "Seems to be depending on what worker a request is handled by.\n",
        "If the worker is currently doing a download, the request will wait until that finishes\n",
        "and most likely time out due to it taking very long.\n",
        "\n",
        "\nThe download and why\n",
        "In the actual SW, the download is a pretty slow transfer of a big stream of data.\n",
        "The source file is similar to an actual file, but has no size. Hence actix-files is not suited for it.\n",
        "There is also the need to check how many downloads are running to ensure ownership rules\n",
        "which the custom TestcaseReader allows with the integrated RefCounter.\n\n",
        "The current file is just a Mock stream of bytes 0-255 repeating. Both cases seem to be affected similarily, so it shouldn't matter",
    ))
}

#[get("/download")]
async fn get_testcase_data() -> Result<impl Responder> {
    // Useful for testing theorectical max speed on the bbb regardless of usb bus max speed.
    //let mut reader = crate::hardware::usb::AsyncMockStream::new(1024 * 1024 * 256);
    //let size = 1024 * 1024 * 256;

    let size = {
        let controller = testcase::controller().await;
        if controller.reading_count() >= 2 {
            //web_error!(500, "<snip (up to 2 allowed at once)>");
        }
        controller.size()
    };

    // <Snip (range header impl)>
    let controller = testcase::controller().await;
    // The BufReader was mainly used for potential performance improvements.
    // Isn't really needed but doesn't seem to affect the problem at all anyway.
    // For range downloads, there would also be used the "take()" function to limit
    // the downloaded size to a user-selected chunk.
    let stream = ReaderStream::new(BufReader::with_capacity(
        256 * 1024,
        controller.get_reader(None).await.unwrap(), //.web_context(500, "<snip (failed to open target for reading)>")?,
    ));
    drop(controller);

    Ok(HttpResponse::Ok()
        .content_type("application/octet-stream")
        .insert_header(("Accept-Ranges", "bytes")) // Range-impl not included in this demo, but would work in the actual sw
        .insert_header(("Content-Disposition", "attachment; filename=target.img"))
        .body(SizedStream::new(size, stream)))
}
