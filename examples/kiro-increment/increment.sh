#!/bin/bash
n=$(cat counter.txt)
echo $((n + 1)) > counter.txt
echo $((n + 1))
