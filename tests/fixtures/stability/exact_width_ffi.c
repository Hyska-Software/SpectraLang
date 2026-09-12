#include <stdint.h>
#include <stddef.h>

#include "spectra_interop.h"

_Static_assert(sizeof(int8_t) == 1, "int8_t must be one byte");
_Static_assert(sizeof(uint8_t) == 1, "uint8_t must be one byte");
_Static_assert(sizeof(int16_t) == 2, "int16_t must be two bytes");
_Static_assert(sizeof(uint16_t) == 2, "uint16_t must be two bytes");
_Static_assert(sizeof(int32_t) == 4, "int32_t must be four bytes");
_Static_assert(sizeof(uint32_t) == 4, "uint32_t must be four bytes");
_Static_assert(sizeof(int64_t) == 8, "int64_t must be eight bytes");
_Static_assert(sizeof(uint64_t) == 8, "uint64_t must be eight bytes");
_Static_assert(sizeof(float) == 4, "float ABI must be IEEE single width");
_Static_assert(sizeof(double) == 8, "double ABI must be IEEE double width");
_Static_assert(offsetof(SpectraF64Array, data) == 0, "array data must be first");

int main(void) {
    int8_t signed_value = spectra_interop_identity_i8(-7);
    uint64_t unsigned_value = spectra_interop_identity_u64(9000000000000000000ULL);
    float single_value = spectra_interop_identity_f32(1.25f);
    double double_value = spectra_interop_identity_f64(2.5);
    return signed_value == -7 && unsigned_value == 9000000000000000000ULL
                   && single_value == 1.25f && double_value == 2.5
               ? 0
               : 1;
}
