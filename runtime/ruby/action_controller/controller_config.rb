# frozen_string_literal: true

# `ApplicationController.config` — Rails' class_attribute reader.
# Assignments land on the process-wide `ControllerConfig` that
# `Base#config` also returns, so `config.relative_url_root =` on the
# class is what an instance method reads back.
module ActionController
  class Base
    def self.config
      ActionController.controller_config
    end
  end
end
