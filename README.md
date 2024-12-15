# Distributed File System: RustFS

## Team Information
- **Team Members**:  
  - Yiren Zhao (1005092427)  
  - Haowei Li (1004793565)  

- **Emails**:  
  - Yiren Zhao: yiren.zhao@example.com  
  - Haowei Li: haowei.li@example.com  


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

### Steps to Reproduce:

1. **Clone the Repository**:
    ```
    git clone https://github.com/Billlhw/RustFS.git
    cd RustFS
    ```
    
2. **Install Dependencies**:  
   Ensure you have Rust installed. Install Rust via [rustup](https://rustup.rs/):
    ```
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
    ```

3. **Compile the Code**:
    ```
    cargo build --release
    ```

4. **Run the System**:
    - Start the master servers and chunkservers as described in the following User’s Guide.
    - Run the client commands to test file operations.
      
## 5. User’s Guide

### 5.1 Prerequisites
- **Operating System**: macOS Sonoma.
- **Dependencies**: 
  - Rust (stable toolchain)
  - Cargo (Rust package manager)
- **Hardware Requirements**: At least 4GB RAM and 10GB storage.

### 5.2 Build and Run the System

#### 5.2.1 Compile Project
   
    cargo build --release
    

#### 5.2.2 Run Master and Chunkservers
1. **Start Master Nodes**:
    ```
    target/release/master -a localhost:50001
    target/release/master -a localhost:50002
    target/release/master -a localhost:50003
    ```

2. **Start Chunkservers**:
    ```bash
    target/release/chunkserver -a localhost:50010
    target/release/chunkserver -a localhost:50011
    ```

#### 5.2.3 Run Client Commands
1. **Upload a File**:
    ```
    target/release/client upload <file_name>
    ```

2. **Read a File**:
    ```
    target/release/client read <file_name>
    ```

3. **Append to a File**:
    ```
    target/release/client append <file_name> "<data>"
    ```

4. **Delete a File**:
    ```
    target/release/client delete <file_name>
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
