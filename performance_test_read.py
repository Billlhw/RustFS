import subprocess
import os
import time
import statistics

def run_command_in_background(command):
    """
    Run a command in the background using subprocess.Popen.
    """
    print(f"Starting command: {command}")
    process = subprocess.Popen(command, shell=True)
    return process

def run_command(command):
    """
    Run a command and wait for it to complete, returning elapsed time.
    """
    print(f"Executing: {command}")
    start_time = time.time()
    result = subprocess.run(command, shell=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    end_time = time.time()
    elapsed_time = end_time - start_time

    if result.returncode != 0:
        print(f"Error: Command failed with error: {result.stderr.decode('utf-8')}")
    else:
        print(f"Success: {result.stdout.decode('utf-8')}")

    return elapsed_time  # Return the time taken for this command

def save_performance_results(file_name, total_time, avg_time, throughput, client_num, file_size_mb, test_type):
    """
    Save the performance results to a file.
    """
    with open(file_name, "w") as f:
        f.write(f"Performance Metrics for {test_type} Test\n")
        f.write(f"Total Files {test_type}: {client_num}\n")
        f.write(f"File Size: {file_size_mb} MB\n")
        f.write(f"Total Time Taken: {total_time:.2f} seconds\n")
        f.write(f"Average {test_type} Time: {avg_time:.2f} seconds\n")
        f.write(f"Throughput: {throughput:.2f} MB/s\n")
    print(f"{test_type} performance results saved to {file_name}")

def main():
    # Step 1: Start Master nodes in the background
    print("Starting Master nodes...")
    master_commands = [
        "target/release/master -a 127.0.0.1:50001",
        "target/release/master -a 127.0.0.1:50002",
        "target/release/master -a 127.0.0.1:50003"
    ]
    master_processes = [run_command_in_background(command) for command in master_commands]

    # Step 2: Start ChunkServer nodes in the background
    print("Starting ChunkServer nodes...")
    chunkserver_commands = [
        "target/release/chunkserver -a 127.0.0.1:50010",
        "target/release/chunkserver -a 127.0.0.1:50011"
    ]
    chunkserver_processes = [run_command_in_background(command) for command in chunkserver_commands]

    # Give processes some time to start before doing any other work
    time.sleep(2)

    # Step 3: Performance Test - Start Clients for upload
    print("Starting upload performance test (uploads)...")
    client_num = 5  # Number of clients for testing
    file_size_mb = 10  # Size of each test file in MB
    upload_times = []  # List to store time taken for each upload

    # Create and upload files for the upload performance test
    for i in range(client_num):
        file_name = f"test_file_upload_{i}.txt"
        # Generate a test file dynamically
        with open(file_name, "wb") as f:
            f.write(os.urandom(file_size_mb * 1024 * 1024))  # Create a file of specified size

        # Upload the test file using a client command
        upload_command = f"target/release/client upload {file_name}"
        elapsed_time = run_command(upload_command)
        upload_times.append(elapsed_time)

        # Cleanup the test file
        os.remove(file_name)
        time.sleep(1)  # Wait between uploads

    # Step 4: Calculate Upload Performance Metrics
    total_time = sum(upload_times)
    avg_time = statistics.mean(upload_times)
    throughput = (client_num * file_size_mb) / total_time  # MB/s

    # Save the upload performance results
    save_performance_results("upload_performance_results.txt", total_time, avg_time, throughput, client_num, file_size_mb, "Upload")

    # Step 5: Performance Test - Start Clients for read
    print("Starting read performance test (reads)...")
    read_times = []  # List to store time taken for each read

    # Generate and upload files for the read performance test
    for i in range(client_num):
        file_name = f"test_file_read_{i}.txt"
        # Create a file and upload it
        with open(file_name, "wb") as f:
            f.write(os.urandom(file_size_mb * 1024 * 1024))  # Create a file of specified size

        # Upload the test file using a client command for read test
        upload_command = f"target/release/client upload {file_name}"
        run_command(upload_command)

        # Now perform the read test for each file
        read_command = f"target/release/client read {file_name}"
        elapsed_time = run_command(read_command)
        read_times.append(elapsed_time)

        # Cleanup the test file
        os.remove(file_name)
        time.sleep(1)  # Wait between reads

    # Step 6: Calculate Read Performance Metrics
    total_time_read = sum(read_times)
    avg_time_read = statistics.mean(read_times)
    throughput_read = (client_num * file_size_mb) / total_time_read  # MB/s

    # Save the read performance results
    save_performance_results("performance_read_results.txt", total_time_read, avg_time_read, throughput_read, client_num, file_size_mb, "Read")

    # Optionally, wait for all processes (Master and ChunkServer) to finish if needed
    for process in master_processes + chunkserver_processes:
        process.wait()

if __name__ == "__main__":
    main()
