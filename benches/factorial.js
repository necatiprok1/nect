function factorial(n) {
    let result = 1;
    for (let i = 1; i <= n; i++) {
        result = result * i;
    }
    return result;
}

const n = 100;
const result = factorial(n);
console.log("factorial(" + n + ") = " + result);