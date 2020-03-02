import os


class FileTooLarge(Exception):
    pass


def stream_to_file(stream, filename, max_size=None, chunk_size=4096):
    os.makedirs(os.path.dirname(filename))

    total_read = 0
    with open(filename, "bw") as f:
        while True:
            chunk = stream.read(chunk_size)
            nbytes = len(chunk)
            if nbytes == 0:
                break

            total_read += nbytes
            if max_size and total_read > max_size:
                raise FileTooLarge

            f.write(chunk)
