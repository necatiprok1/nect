#include <math.h>
#include <string.h>

double ffi_add(double a, double b) { return a + b; }
double ffi_multiply(double a, double b) { return a * b; }
double ffi_sqrt_val(double a) { return sqrt(a); }
double ffi_pow_val(double base, double exp) { return pow(base, exp); }
double ffi_pi(void) { return 3.141592653589793; }
double ffi_max_val(double a, double b, double c, double d) {
    double m = a;
    if (b > m) m = b;
    if (c > m) m = c;
    if (d > m) m = d;
    return m;
}

/* String interop: counts characters in a string (returns double) */
double ffi_string_length(const char *s) { return (double)strlen(s); }

/* String interop: returns a greeting string */
const char *ffi_greeting(void) { return "Hello from C!"; }

/* String interop: concatenates two strings (returns length of result) */
double ffi_string_concat(const char *a, const char *b) {
    size_t la = strlen(a);
    size_t lb = strlen(b);
    return (double)(la + lb);
}

/* String interop: returns uppercase of a single-char string */
const char *ffi_get_const_string(void) { return "constant"; }

