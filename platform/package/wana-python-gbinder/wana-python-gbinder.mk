################################################################################
#
# wana-python-gbinder
#
################################################################################

WANA_PYTHON_GBINDER_VERSION = 86b8feba4cacd0952b010d1c3af6a29a0c146ced
WANA_PYTHON_GBINDER_SITE = https://github.com/waydroid/gbinder-python.git
WANA_PYTHON_GBINDER_SITE_METHOD = git
WANA_PYTHON_GBINDER_SETUP_TYPE = setuptools
WANA_PYTHON_GBINDER_LICENSE = GPL-3.0
WANA_PYTHON_GBINDER_LICENSE_FILES = LICENSE
WANA_PYTHON_GBINDER_DEPENDENCIES = host-pkgconf host-python-cython wana-libgbinder

$(eval $(python-package))
