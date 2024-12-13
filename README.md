# RustFS

A Distributed File System implemented in Rust.

## Overview

RustFS is a simple distributed file system built using Rust and gRPC. It allows clients to upload, retrieve, update, and delete files on a remote server. This project demonstrates the use of Rust's asynchronous programming capabilities and gRPC for client-server communication.

## Features

- **Upload**: Stream files from the client to the server.
- **Get**: Retrieve the content of files stored on the server.
- **Append**: Append to the end of existing files on the server.
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
   ./client upload example.txt
