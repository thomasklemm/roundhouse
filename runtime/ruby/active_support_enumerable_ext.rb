# ActiveSupport Enumerable extensions that need a shared implementation
# instead of reopening a built-in that native targets cannot dispatch on.
module ActiveSupport
  # Rails' zero-argument Enumerable#index_with block form. A new Hash is
  # returned; duplicate source values overwrite earlier values as in Rails.
  def self.index_with(list)
    result = {}
    list.each { |item| result[item] = yield item }
    result
  end
end
