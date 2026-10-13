# Prints the built-in level tables of the Across 1.2 Windows port, which
# crates/elma-formats/src/across/table.rs copies: python3 -I scripts/across-tables.py across_w.exe
import struct
import sys

data = open(sys.argv[1], "rb").read()

# Leveltomb: {int ujpalya; int leiroazonosito; char description[40]; char file[12]}
LEVELTOMB = 0x432F0
for i in range(74):
    offset = LEVELTOMB + 60 * i
    _, descriptor = struct.unpack_from("<ii", data, offset)
    name = data[offset + 8 : offset + 48].split(b"\0")[0].decode("latin1")
    file = data[offset + 48 : offset + 60].split(b"\0")[0].decode("latin1")
    print(f'e("{name}", "{file}", {descriptor}),')

# Levellista: int[90]
LEVELLISTA = 0x44A60
print(list(struct.unpack_from("<90i", data, LEVELLISTA)))
