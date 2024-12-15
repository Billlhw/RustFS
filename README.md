# Distributed File System: RustFS

## Team Information 
- **Yiren Zhao** (1005092427): yiren.zhao@mail.utoronto.ca
- **Haowei Li** (1004793565): haowei.li@mail.utoronto.ca


## 1. Motivation
Distributed file systems are the backbone of modern data-driven applications, offering scalability, reliability, and high availability. With Rust's rise as a systems programming language, its ecosystem has grown to include high-performance and resilient storage systems. However, many of these implementations lack advanced features such as end-to-end encryption, real-time file tailing, and robust fault tolerance mechanisms.

Our motivation stems from a desire to fill this gap and leverage Rust's concurrency and memory safety to create a state-of-the-art distributed file system. Inspired by the Google File System (GFS), our system incorporates advanced functionality tailored for modern distributed environments, making it secure, reliable, and user-friendly. This project provided us with an opportunity to explore distributed systems concepts while addressing a real-world need for enhanced distributed storage capabilities in the Rust ecosystem.

## 2. Objectives
The objective of our project is to develop a scalable, high-available, and high-performance file storage system. Our system aims to support growing data and user demands by allowing on-demand addition of storage nodes without disrupting existing operations. To ensure high availability and durability, the system is resilient to component failures and supports automatic detection and recovery of node failures. Efficiency is further optimized through client-side metadata caching, and planning of dataflow during mutations. Additionally, we will incorporate advanced functionality, including access control, end-to-end encryption, and file tailing.



## 3. Features

### 3.1 Core Features
- **Centralized Metadata Management**: A master node maintains and replicates metadata, ensuring consistency and durability across the system.
- **Data Replication**: Files are replicated across multiple chunkservers to guarantee availability and fault tolerance.
- **Replica Consistency Management**: A lease mechanism ensures that mutations are applied in the same order across replicas.
- **Scalable Architecture**: The system supports horizontal scaling by adding new chunkservers without service disruption.

### 3.2 Advanced Features
- **End-to-End Encryption**: Secure data transmission using asymmetric encryption for key exchange and symmetric encryption for file chunks.
- **Real-Time File Tailing**: Enables real-time updates to append-only files, useful for log tracking.
- **User Authentication**: Validates client credentials and issues session tokens for secure communication.
- **Automated Failure Recovery**: Detects master and chunkserver failures and initiates rebalancing or leader election for recovery.

## 4. Reproducibility Guide
This guide explains how to clone, set up, build RustFS step-by-step, ensuring a working distributed file system on your local machine.

### 4.1 Prerequisites
Before proceeding, ensure the following requirements are met:

#### Operating System
- **Supported OS**: macOS Sonoma.

#### Rust Installation
RustFS requires the Rust programming language and its package manager, Cargo. Install Rust using the official [rustup installer](https://rustup.rs/):
```
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Once installed, verify the installation:
```
rustc --version
cargo --version
```
Ensure Rust version is **1.70+**.

#### Hardware Requirements
At least 4GB RAM and 10GB storage.

### 4.2 Setting Up RustFS

**Step 1: Clone the Repository**

Clone the RustFS repository from GitHub to your local machine:
```
git clone https://github.com/Billlhw/RustFS.git
cd RustFS
```
    
**Step 2: Review the Configuration**

Ensure the `config.toml` file is properly set up. This file contains key settings for master nodes, chunkservers, and clients.

Here’s a sample `config.toml`
```
[master]
log_path = "logs"
cron_interval = 5
heartbeat_failure_threshold = 2

[chunkserver]
data_path = "data"
log_path = "logs"

[client]
log_path = "client/logs"

[common]
master_addrs = ["127.0.0.1:50001", "127.0.0.1:50002", "127.0.0.1:50003"]
heartbeat_interval = 10
chunk_size = 4096
max_allowed_chunks = 100
replication_factor = 2
log_level = "info"
log_output = "stdout"
```
Make sure the `master_addrs` lists all master nodes and that `data_path` is a writable directory for chunkservers.

**Step 3: Build the System**

Compile RustFS in release mode:
```
cargo build --release
```

This will generate the following binaries:

- `target/release/master`: Master node executable
- `target/release/chunkserver`: Chunkserver executable
- `target/release/client`: Client executable

Ensure these files exist:

```
ls target/release/master target/release/chunkserver target/release/client
```

### 4.3 Troubleshooting

**Issue: Compilation Errors**
- Ensure you have the latest Rust toolchain installed.
- Run cargo clean and recompile the project:
  ```
  cargo clean
  cargo build --release
  ```

**Issue: Chunkserver Not Registering**
- Verify the ```master_addrs``` in ```config.toml``` is correct.
- Check if the master node is running.

**Issue: Issue: File Operations Fail**
- Ensure all chunkservers and master nodes are running.
- Check logs for error messages.
  
      
## 5. Running RustFS Guide
You need to start the **master nodes**, **chunkservers**, and then use the **client** to interact with the system.

### Step 1: Start Master Nodes
    Start multiple master nodes (as part of leader-election and fault tolerance). Run each of the following commands in a separate terminal window:
    ```
    target/release/master -a localhost:50001
    target/release/master -a localhost:50002
    target/release/master -a localhost:50003
    ```
    Verify the output in each terminal. One of the nodes will elect itself as the leader.

### Step 2: Start Chunkservers
Start chunkservers that will store file data. Run each chunkserver in separate terminals:
```
target/release/chunkserver -a localhost:50010
target/release/chunkserver -a localhost:50011
```
Ensure the `data_path` directory exists for storing chunk data:
```
mkdir -p data
```
Verify chunkserver logs to ensure they successfully register with the master node.

### Step 3: Test the System Using the Client

Once the master nodes and chunkservers are running, use the client to perform file operations. Basic operations including uploading, reading, appending, and deleting files. The following examples demonstrate these operations using ```example.txt``` as the ```<file_name>```:

### Command 1: Upload a File

Upload a file to the distributed file system:

```
target/release/client upload <file_name>
```

Expected output:
```
Uploading <file_name>...
File successfully uploaded.
```


### Command 2: Read a File
Read the contents of a file stored in the system:
```
target/release/client read <file_name>
```
Expected output:
```
Reading <file_name>...
File contents:
[contents of <file_name>]
```

### Command 3: Append to a File
Append data to the end of an existing file:

```
target/release/client append <file_name> "<data>"
```
Expected output:
```
Appending data to <file_name>...
Append successful.
```

### Command 4: Delete a File
Delete a file from the system:
```
target/release/client delete <file_name>
```
Expected output:
```
Deleting <file_name>...
File successfully deleted.
```

## 6. Contributions by Team Members
- **Yiren Zhao**:
  - Developed the chunkserver logic, including data storage and replication mechanisms.
  - Implemented core file operations such as upload, read, append, and delete.
  - Designed and tested real-time file tailing.

- **Haowei Li**:
  - Developed the master node logic, including metadata management and load balancing.
  - Implemented fault detection and recovery mechanisms for master and chunkservers.
  - Designed and implemented the user authentication and encryption mechanisms.

## 7. Lessons Learned and Concluding Remarks

### Lessons Learned
Throughout this project, we gained invaluable insights into distributed systems, fault tolerance, and the challenges of ensuring consistency in a distributed environment. Working with Rust deepened our understanding of memory safety and concurrency. The project demonstrated the importance of designing for scalability and modularity, as we were able to add advanced features without disrupting the core system.

We believe this project lays a strong foundation for further exploration of distributed file systems in Rust, potentially serving as a base for more advanced research or commercial implementations. Future work could include integrating machine learning for predictive load balancing and extending the system to support object storage for cloud environments.

### Concluding Remarks
This project successfully combines core file storage functionalities with advanced features like encryption, file tailing, and fault tolerance. The system is scalable, secure, and resilient to failures. Future work could include optimizing the metadata handling for large-scale deployments and adding support for cross-rack replication.

## 8. References
[1] Ghemawat, Sanjay, Howard Gobioff, and Shun-Tak Leung. "The Google file system." Proceedings of the nineteenth ACM symposium on Operating systems principles. 2003.

[2] Crust Network. Crust: Decentralized Storage Network for Web3. GitHub, n.d., https://github.com/crustio/crust. Accessed 3 Nov. 2024.

## **License**
This project is licensed under the MIT License. See the LICENSE file for more details.
