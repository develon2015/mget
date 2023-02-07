# mget
多线程下载工具

* 支持HTTP代理
* 默认伪造Chrome浏览器UA
* 支持cookies.txt
* 随时取消/继续下载


用法：

```
mget -t 3 -p http://localhost:25378 -o out.bin http://example.com/res.bin -coo cookies.txt
```

等价于：

```
mget http://example.com/res.bin --thread 3 --proxy http://localhost:25378 --output out.bin --cookies cookies.txt
```

继续之前中断的下载：

```
mget -c out.bin.mget
```

以新的URL继续之前中断的下载：

```
$ mget -c out.bin.mget http://example.com/res.bin?new=true
query resource...
content_length: 50000000
[2187786, 18843581, 35510263] Progress:  13.08%  6.24MB/47.68MB  1.89MB/s
```
