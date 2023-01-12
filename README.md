# mget
多线程下载工具

* 支持HTTP代理
* 默认伪造Chrome浏览器UA
* ~~支持cookies.txt~~

```
mget -t 3 -p http://localhost:25378 -o out.bin http://example.com/res.bin
```

also

```
mget --thread 3 --proxy http://localhost:25378 --output out.bin http://example.com/res.bin
```
