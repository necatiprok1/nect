const n: number = 100000;
let sum: number = 0;
for (let i: number = 0; i < n; i++) {
    sum = sum + i;
}
console.log("sum(1.." + n + ") = " + sum);