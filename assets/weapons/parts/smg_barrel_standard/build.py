"""The issue shroud: collar, finned jacket, crowned tip. (A part variant: the geometry is in ../gunparts.py.)"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import gunparts  # noqa: E402

gunparts.build(__file__)
