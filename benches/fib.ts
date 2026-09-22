function fib(n: number): number {
    if (n <= 1) {
        return n;
    }
    return fib(n - 1) + fib(n - 2);
}

const n: number = 30;
const result: number = fib(n);
console.log("fib(" + n + ") = " + result);