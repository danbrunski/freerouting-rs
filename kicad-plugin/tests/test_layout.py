"""layout.py: how far the route dialog's tab pages are cut so that the dialog
fits on the screen."""

import layout


def test_a_dialog_that_fits_is_not_cut():
    assert layout.cut_to_screen([500, 300, 420], dialog_height=800, max_height=900) is None
    assert layout.cut_to_screen([500, 300, 420], dialog_height=900, max_height=900) is None


def test_the_tallest_page_shrinks_by_the_excess_and_shorter_ones_down_to_it():
    # 1200 high on a 900 screen: the notebook (as tall as its tallest page,
    # 700) must lose 300, so every page is at most 400.
    assert layout.cut_to_screen([700, 300, 520], dialog_height=1200, max_height=900) == [400, 300, 400]


def test_no_page_is_cut_below_the_floor():
    assert layout.cut_to_screen([700, 100], dialog_height=1600, max_height=900, floor=160) == [160, 100]


def test_no_pages_means_nothing_to_cut():
    assert layout.cut_to_screen([], dialog_height=1600, max_height=900) is None


def test_the_dialog_may_take_most_of_the_work_area():
    assert layout.max_dialog_height(1040) == 936

