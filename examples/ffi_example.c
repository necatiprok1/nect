/* ffi_example.c - C library for the FFI example.
 * Build: cc -shared -fPIC -o libexample.so ffi_example.c
 */
#include <math.h>

double ffi_add(double a, double b) { return a + b; }
double ffi_multiply(double a, double b) { return a * b; }
double ffi_compute(double base, double exp) { return pow(base, exp) + 1.0; }
