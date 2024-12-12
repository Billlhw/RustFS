# RustFS

A Distributed File System implemented in Rust.

## Overview

RustFS is a simple distributed file system built using Rust and gRPC. It allows clients to upload, retrieve, update, and delete files on a remote server. This project demonstrates the use of Rust's asynchronous programming capabilities and gRPC for client-server communication.

## Features

- **Upload**: Stream files from the client to the server.
- **Get**: Retrieve the content of files stored on the server.
- **Update**: Modify the content of existing files on the server.
- **Delete**: Remove files from the server.

## Setup

1. Build and run

   ```bash
   cargo build
   # Run server
   cargo run --bin server
   # Run client
   cargo run --bin client

2. Sample client command

   ```bash
    Client> upload example.txt
    Upload Response: File 'example.txt' uploaded successfully.
    Client> read example.txt
    File Content:
    Hello, World!

    Client> exit
    Exiting client.