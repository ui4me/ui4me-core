import struct, sys
with open(sys.argv[1], "rb") as f:
    d = f.read(4096)
pe = struct.unpack_from("<I", d, 0x3c)[0]
assert d[pe:pe+4] == b"PE\0\0"
opt = pe + 24
magic = struct.unpack_from("<H", d, opt)[0]
if magic == 0x20b:  # PE32+
    reserve, commit = struct.unpack_from("<QQ", d, opt + 72)
else:
    reserve, commit = struct.unpack_from("<II", d, opt + 72)
print(f"magic={hex(magic)} SizeOfStackReserve={reserve} ({reserve/1024/1024:.2f} MiB) commit={commit}")
